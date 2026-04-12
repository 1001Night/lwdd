use anyhow::Result;
use clap::Parser;
use common::{Message, DISCOVERY_PORT, HEARTBEAT_INTERVAL_SECS};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time;

#[derive(Parser, Debug)]
#[command(name = "ddns-client")]
#[command(about = "Dynamic DNS Client")]
struct Args {
    #[arg(short, long, help = "Custom hostname (defaults to system hostname)")]
    hostname: Option<String>,

    #[arg(short, long, help = "Subnet to scan (e.g., 192.168.0)")]
    subnet: Option<String>,

    #[arg(long, help = "Server IP address (skip subnet scan)")]
    server: Option<String>,
}

async fn scan_subnet(subnet: &str) -> Result<SocketAddr> {
    println!("Сканирование подсети {}...", subnet);

    let mut tasks = vec![];

    for i in 1..=254 {
        let ip = format!("{}.{}", subnet, i);
        let task = tokio::spawn(async move {
            let addr = format!("{}:{}", ip, DISCOVERY_PORT);
            if let Ok(addr) = addr.parse::<SocketAddr>() {
                if let Ok(Ok(mut stream)) = tokio::time::timeout(
                    Duration::from_millis(1000),
                    TcpStream::connect(addr)
                ).await {
                    if let Ok(()) = send_discovery(&mut stream).await {
                        return Some(addr);
                    }
                }
            }
            None
        });
        tasks.push(task);
    }

    for task in tasks {
        if let Ok(Some(addr)) = task.await {
            println!("Найден сервер: {}", addr);
            return Ok(addr);
        }
    }

    anyhow::bail!("Сервер не найден в подсети {}", subnet)
}

async fn send_discovery(stream: &mut TcpStream) -> Result<()> {
    let msg = Message::DiscoveryRequest;
    let bytes = msg.to_bytes()?;

    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;

    let len = stream.read_u32().await?;
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;

    match Message::from_bytes(&buf)? {
        Message::DiscoveryResponse => Ok(()),
        _ => anyhow::bail!("Неожиданный ответ"),
    }
}

async fn get_local_ip(subnet: Option<&str>) -> Result<IpAddr> {
    if let Some(subnet) = subnet {
        #[cfg(unix)]
        {
            use std::process::Command;
            if let Ok(output) = Command::new("ip").args(&["-4", "addr", "show"]).output() {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    if line.contains("inet ") && !line.contains("127.0.0.1") {
                        if let Some(ip_part) = line.split_whitespace().nth(1) {
                            if let Some(ip_str) = ip_part.split('/').next() {
                                if ip_str.starts_with(subnet) {
                                    if let Ok(addr) = ip_str.parse::<IpAddr>() {
                                        return Ok(addr);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect("8.8.8.8:80").await?;
    let addr = socket.local_addr()?;
    Ok(addr.ip())
}

fn get_hostname() -> Result<String> {
    #[cfg(unix)]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("hostnamectl").arg("hostname").output() {
            let hostname = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !hostname.is_empty() {
                return Ok(hostname);
            }
        }
        if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
            let hostname = hostname.trim().to_string();
            if !hostname.is_empty() {
                return Ok(hostname);
            }
        }
    }

    #[cfg(windows)]
    {
        if let Ok(name) = std::env::var("COMPUTERNAME") {
            return Ok(name);
        }
    }

    anyhow::bail!("Cannot get hostname")
}

async fn heartbeat_loop(server_addr: SocketAddr, hostname: String, ip: IpAddr) -> Result<()> {
    let mut interval = time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));

    loop {
        interval.tick().await;

        match send_heartbeat(server_addr, &hostname, ip).await {
            Ok(_) => println!("Heartbeat отправлен: {} -> {}", hostname, ip),
            Err(e) => eprintln!("Ошибка отправки heartbeat: {}", e),
        }
    }
}

async fn send_heartbeat(server_addr: SocketAddr, hostname: &str, ip: IpAddr) -> Result<()> {
    let mut stream = TcpStream::connect(server_addr).await?;

    let msg = Message::Heartbeat {
        hostname: hostname.to_string(),
        ip,
    };
    let bytes = msg.to_bytes()?;

    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;

    let len = stream.read_u32().await?;
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;

    match Message::from_bytes(&buf)? {
        Message::HeartbeatAck => Ok(()),
        _ => anyhow::bail!("Неожиданный ответ на heartbeat"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let hostname = args.hostname.unwrap_or_else(|| {
        get_hostname().unwrap_or_else(|_| "unknown".to_string())
    });

    let subnet_ref = args.subnet.as_deref();
    let ip = get_local_ip(subnet_ref).await?;

    let subnet = args.subnet.unwrap_or_else(|| {
        if let IpAddr::V4(ipv4) = ip {
            let octets = ipv4.octets();
            format!("{}.{}.{}", octets[0], octets[1], octets[2])
        } else {
            "192.168.0".to_string()
        }
    });

    let server_addr = if let Some(server_ip) = args.server {
        format!("{}:{}", server_ip, DISCOVERY_PORT).parse()?
    } else {
        scan_subnet(&subnet).await?
    };

    println!("Подключено к серверу: {}", server_addr);
    println!("Hostname: {}", hostname);
    println!("IP: {}", ip);

    heartbeat_loop(server_addr, hostname, ip).await?;

    Ok(())
}
