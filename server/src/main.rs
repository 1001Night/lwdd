use anyhow::Result;
use clap::Parser;
use common::{Message, DISCOVERY_PORT, DNS_PORT};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use hickory_resolver::TokioAsyncResolver;
use hickory_resolver::config::{ResolverConfig, ResolverOpts, NameServerConfig, Protocol};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::RwLock;
use tokio::time;

#[derive(Parser, Debug)]
#[command(name = "ddns-server")]
#[command(about = "Dynamic DNS Server")]
struct Args {
    #[arg(short, long, help = "Subnet to scan on startup (e.g., 192.168.0)")]
    subnet: Option<String>,

    #[arg(short, long, help = "Domain suffix for DNS (e.g., local)")]
    domain: Option<String>,

    #[arg(short, long, default_value_t = DNS_PORT, help = "DNS server port (default: 53)")]
    port: u16,
}

#[derive(Clone)]
struct DnsRecord {
    ip: IpAddr,
    last_seen: SystemTime,
}

type DnsRegistry = Arc<RwLock<HashMap<String, DnsRecord>>>;

async fn scan_subnet_for_clients(subnet: &str, _registry: DnsRegistry) {
    println!("Сканирование подсети {} для поиска клиентов...", subnet);

    let mut tasks = vec![];

    for i in 1..=254 {
        let ip = format!("{}.{}", subnet, i);

        let task = tokio::spawn(async move {
            let addr = format!("{}:{}", ip, DISCOVERY_PORT);
            if let Ok(addr) = addr.parse::<SocketAddr>() {
                if let Ok(Ok(_stream)) = tokio::time::timeout(
                    Duration::from_millis(100),
                    TcpStream::connect(addr)
                ).await {
                    println!("Обнаружен потенциальный клиент: {}", addr);
                }
            }
        });
        tasks.push(task);
    }

    for task in tasks {
        let _ = task.await;
    }

    println!("Сканирование завершено");
}

async fn handle_client(mut stream: TcpStream, registry: DnsRegistry) -> Result<()> {
    let len = stream.read_u32().await?;
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;

    let msg = Message::from_bytes(&buf)?;

    match msg {
        Message::DiscoveryRequest => {
            let response = Message::DiscoveryResponse;
            let bytes = response.to_bytes()?;
            stream.write_u32(bytes.len() as u32).await?;
            stream.write_all(&bytes).await?;
        }
        Message::Heartbeat { hostname, ip } => {
            let mut registry = registry.write().await;
            registry.insert(
                hostname.clone(),
                DnsRecord {
                    ip,
                    last_seen: SystemTime::now(),
                },
            );
            println!("Heartbeat получен: {} -> {}", hostname, ip);

            let response = Message::HeartbeatAck;
            let bytes = response.to_bytes()?;
            stream.write_u32(bytes.len() as u32).await?;
            stream.write_all(&bytes).await?;
        }
        _ => {}
    }

    Ok(())
}

async fn tcp_server(registry: DnsRegistry) -> Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", DISCOVERY_PORT)).await?;
    println!("TCP сервер запущен на порту {}", DISCOVERY_PORT);

    loop {
        let (stream, addr) = listener.accept().await?;
        let registry = registry.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, registry).await {
                eprintln!("Ошибка обработки клиента {}: {}", addr, e);
            }
        });
    }
}

async fn cleanup_stale_records(registry: DnsRegistry) {
    let mut interval = time::interval(Duration::from_secs(60));

    loop {
        interval.tick().await;

        let mut registry = registry.write().await;
        let now = SystemTime::now();

        registry.retain(|hostname, record| {
            if let Ok(elapsed) = now.duration_since(record.last_seen) {
                if elapsed > Duration::from_secs(90) {
                    println!("Удаление устаревшей записи: {}", hostname);
                    return false;
                }
            }
            true
        });
    }
}

struct DynamicDnsHandler {
    registry: DnsRegistry,
    domain_suffix: String,
    resolver: TokioAsyncResolver,
}

impl DynamicDnsHandler {
    async fn lookup(&self, name: &Name, rtype: RecordType) -> Option<Vec<Record>> {
        if rtype != RecordType::A {
            return None;
        }

        let name_str = name.to_utf8();
        let hostname = if name_str.ends_with(&format!(".{}.", self.domain_suffix)) {
            name_str
                .strip_suffix(&format!(".{}.", self.domain_suffix))
                .unwrap_or(&name_str)
        } else {
            name_str.trim_end_matches('.')
        };

        let registry = self.registry.read().await;

        if let Some(record) = registry.get(hostname) {
            if let IpAddr::V4(ipv4) = record.ip {
                let mut dns_record = Record::new();
                dns_record.set_name(name.clone());
                dns_record.set_rr_type(RecordType::A);
                dns_record.set_ttl(30);
                dns_record.set_data(Some(RData::A(ipv4.into())));

                return Some(vec![dns_record]);
            }
        }

        drop(registry);

        if let Ok(response) = self.resolver.lookup_ip(name_str).await {
            let mut records = Vec::new();
            for ip in response.iter() {
                if let IpAddr::V4(ipv4) = ip {
                    let mut dns_record = Record::new();
                    dns_record.set_name(name.clone());
                    dns_record.set_rr_type(RecordType::A);
                    dns_record.set_ttl(300);
                    dns_record.set_data(Some(RData::A(ipv4.into())));
                    records.push(dns_record);
                }
            }
            if !records.is_empty() {
                return Some(records);
            }
        }

        None
    }
}

async fn dns_server(registry: DnsRegistry, domain_suffix: String, port: u16) -> Result<()> {
    use hickory_proto::op::{MessageType, OpCode, ResponseCode};
    use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};

    let socket = Arc::new(UdpSocket::bind(format!("0.0.0.0:{}", port)).await?);
    println!("DNS сервер запущен на порту {}", port);

    let mut resolver_config = ResolverConfig::new();
    for ip in ["94.140.14.15", "94.140.14.16", "1.1.1.1", "1.0.0.1"] {
        resolver_config.add_name_server(NameServerConfig {
            socket_addr: SocketAddr::new(ip.parse().unwrap(), 53),
            protocol: Protocol::Udp,
            tls_dns_name: None,
            trust_negative_responses: true,
            bind_addr: None,
        });
    }

    let resolver = TokioAsyncResolver::tokio(resolver_config, ResolverOpts::default());

    let handler = Arc::new(DynamicDnsHandler {
        registry,
        domain_suffix,
        resolver,
    });

    let mut buf = vec![0u8; 512];

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await?;
        let request_bytes = buf[..len].to_vec();
        let handler = handler.clone();
        let socket = socket.clone();

        tokio::spawn(async move {
            if let Ok(request) = hickory_proto::op::Message::from_bytes(&request_bytes) {
                let mut response = hickory_proto::op::Message::new();
                response.set_id(request.id());
                response.set_message_type(MessageType::Response);
                response.set_op_code(OpCode::Query);
                response.set_recursion_desired(request.recursion_desired());
                response.set_recursion_available(false);

                if let Some(query) = request.queries().first() {
                    response.add_query(query.clone());

                    if let Some(records) = handler.lookup(query.name(), query.query_type()).await {
                        response.set_response_code(ResponseCode::NoError);
                        for record in records {
                            response.add_answer(record);
                        }
                        println!("DNS запрос: {} -> найдено", query.name());
                    } else {
                        response.set_response_code(ResponseCode::NXDomain);
                        println!("DNS запрос: {} -> не найдено", query.name());
                    }
                } else {
                    response.set_response_code(ResponseCode::FormErr);
                }

                if let Ok(response_bytes) = response.to_bytes() {
                    let _ = socket.send_to(&response_bytes, addr).await;
                }
            }
        });
    }
}

fn get_local_subnet() -> String {
    use std::process::Command;

    #[cfg(unix)]
    {
        if let Ok(output) = Command::new("ip").args(&["-4", "addr", "show"]).output() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.contains("inet ") && !line.contains("127.0.0.1") {
                    if let Some(ip_part) = line.split_whitespace().nth(1) {
                        if let Some(ip_str) = ip_part.split('/').next() {
                            if let Ok(addr) = ip_str.parse::<Ipv4Addr>() {
                                let octets = addr.octets();
                                if octets[0] != 172 && octets[0] != 10 || (octets[0] == 10 && octets[1] < 100) {
                                    return format!("{}.{}.{}", octets[0], octets[1], octets[2]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    "192.168.0".to_string()
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let registry: DnsRegistry = Arc::new(RwLock::new(HashMap::new()));
    let domain_suffix = args.domain.unwrap_or_else(|| "local".to_string());
    let dns_port = args.port;

    let subnet = args.subnet.unwrap_or_else(|| get_local_subnet());

    println!("Dynamic DNS Server");
    println!("Домен: .{}", domain_suffix);
    println!("DNS порт: {}", dns_port);
    println!("Подсеть: {}.0/24", subnet);

    let registry_clone = registry.clone();
    tokio::spawn(async move {
        scan_subnet_for_clients(&subnet, registry_clone).await;
    });

    let registry_clone = registry.clone();
    tokio::spawn(async move {
        cleanup_stale_records(registry_clone).await;
    });

    let registry_clone = registry.clone();
    tokio::spawn(async move {
        if let Err(e) = tcp_server(registry_clone).await {
            eprintln!("Ошибка TCP сервера: {}", e);
        }
    });

    dns_server(registry, domain_suffix, dns_port).await?;

    Ok(())
}
