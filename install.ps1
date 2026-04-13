# Требует запуск от администратора
if (-NOT ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole] "Administrator")) {
    Write-Error "Запусти PowerShell от администратора"
    exit 1
}

$InstallDir = "$env:ProgramFiles\LDDNS"
$ConfigDir = "$env:ProgramData\LDDNS"

Write-Host "Установка LDDNS..." -ForegroundColor Green

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path $ConfigDir | Out-Null

if (Test-Path ".\lddns-client-windows.exe") {
    Copy-Item ".\lddns-client-windows.exe" "$InstallDir\lddns-client.exe"
    Write-Host "✓ Клиент установлен" -ForegroundColor Green
}

if (Test-Path ".\lddns-server-windows.exe") {
    Copy-Item ".\lddns-server-windows.exe" "$InstallDir\lddns-server.exe"
    Write-Host "✓ Сервер установлен" -ForegroundColor Green
}

$env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine")
if ($env:Path -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$env:Path;$InstallDir", "Machine")
    Write-Host "✓ Добавлено в PATH" -ForegroundColor Green
}

@"
SERVER=auto
HOSTNAME=$env:COMPUTERNAME
ENABLED=false
"@ | Out-File -FilePath "$ConfigDir\client.conf" -Encoding UTF8

Write-Host "✓ Конфиг создан" -ForegroundColor Green

$ServiceScript = @"
`$ConfigFile = "$ConfigDir\client.conf"
`$Config = Get-Content `$ConfigFile | ConvertFrom-StringData
`$Server = `$Config.SERVER
`$Hostname = `$Config.HOSTNAME

if (`$Server -eq "auto") {
    `$Server = (Get-NetIPAddress -AddressFamily IPv4 | Where-Object {`$_.InterfaceAlias -notlike "*Loopback*"} | Select-Object -First 1).IPAddress
}

& "$InstallDir\lddns-client.exe" --hostname `$Hostname --server `$Server
"@

$ServiceScript | Out-File -FilePath "$InstallDir\lddns-client-service.ps1" -Encoding UTF8

nssm install LDDNS-Client "powershell.exe" "-ExecutionPolicy Bypass -File `"$InstallDir\lddns-client-service.ps1`""
nssm set LDDNS-Client Start SERVICE_AUTO_START
nssm set LDDNS-Client AppStdout "$ConfigDir\client.log"
nssm set LDDNS-Client AppStderr "$ConfigDir\client-error.log"

Write-Host "✓ Сервис создан" -ForegroundColor Green

$LddnsScript = @'
param(
    [Parameter(Position=0)]
    [string]$Command,

    [Parameter(Position=1)]
    [string]$Arg1,

    [Parameter(Position=2)]
    [string]$Arg2
)

$ConfigFile = "$env:ProgramData\LDDNS\client.conf"
$BackupDNS = "$env:ProgramData\LDDNS\dns-backup.txt"

function Enable-LDDNS {
    param([string]$ServerIP = "auto")

    if ($ServerIP -eq "auto") {
        $ServerIP = (Get-NetIPAddress -AddressFamily IPv4 | Where-Object {$_.InterfaceAlias -notlike "*Loopback*"} | Select-Object -First 1).IPAddress
    }

    $Adapters = Get-NetAdapter | Where-Object {$_.Status -eq "Up"}

    foreach ($Adapter in $Adapters) {
        $CurrentDNS = (Get-DnsClientServerAddress -InterfaceIndex $Adapter.ifIndex -AddressFamily IPv4).ServerAddresses
        "$($Adapter.ifIndex)|$($CurrentDNS -join ',')" | Out-File -Append $BackupDNS

        Set-DnsClientServerAddress -InterfaceIndex $Adapter.ifIndex -ServerAddresses $ServerIP
    }

    (Get-Content $ConfigFile) -replace '^ENABLED=.*', 'ENABLED=true' | Set-Content $ConfigFile
    (Get-Content $ConfigFile) -replace '^SERVER=.*', "SERVER=$ServerIP" | Set-Content $ConfigFile

    Start-Service LDDNS-Client

    Write-Host "✓ LDDNS включен (DNS: $ServerIP)" -ForegroundColor Green
}

function Disable-LDDNS {
    Stop-Service LDDNS-Client

    if (Test-Path $BackupDNS) {
        Get-Content $BackupDNS | ForEach-Object {
            $Parts = $_ -split '\|'
            $IfIndex = $Parts[0]
            $DNS = $Parts[1] -split ','

            if ($DNS) {
                Set-DnsClientServerAddress -InterfaceIndex $IfIndex -ServerAddresses $DNS
            }
        }
        Remove-Item $BackupDNS
    } else {
        $Adapters = Get-NetAdapter | Where-Object {$_.Status -eq "Up"}
        foreach ($Adapter in $Adapters) {
            Set-DnsClientServerAddress -InterfaceIndex $Adapter.ifIndex -ServerAddresses @("8.8.8.8", "1.1.1.1")
        }
    }

    (Get-Content $ConfigFile) -replace '^ENABLED=.*', 'ENABLED=false' | Set-Content $ConfigFile

    Write-Host "✓ LDDNS отключен (DNS восстановлен)" -ForegroundColor Green
}

function Get-LDDNSStatus {
    $Service = Get-Service LDDNS-Client -ErrorAction SilentlyContinue

    if ($Service -and $Service.Status -eq "Running") {
        Write-Host "LDDNS: активен" -ForegroundColor Green
        $DNS = (Get-DnsClientServerAddress -AddressFamily IPv4 | Select-Object -First 1).ServerAddresses
        Write-Host "DNS сервер: $($DNS -join ', ')"
    } else {
        Write-Host "LDDNS: неактивен" -ForegroundColor Yellow
    }
}

function Get-LDDNSConfig {
    param([string]$Key, [string]$Value)

    if ($Key -and $Value) {
        (Get-Content $ConfigFile) -replace "^$Key=.*", "$Key=$Value" | Set-Content $ConfigFile
        Write-Host "✓ $Key=$Value" -ForegroundColor Green
    } else {
        Get-Content $ConfigFile
    }
}

switch ($Command) {
    "enable" { Enable-LDDNS -ServerIP $Arg1 }
    "disable" { Disable-LDDNS }
    "status" { Get-LDDNSStatus }
    "config" { Get-LDDNSConfig -Key $Arg1 -Value $Arg2 }
    default {
        Write-Host "Использование: lddns {enable|disable|status|config [KEY VALUE]}"
        Write-Host ""
        Write-Host "  enable [SERVER_IP]  - Включить LDDNS"
        Write-Host "  disable             - Отключить LDDNS"
        Write-Host "  status              - Показать статус"
        Write-Host "  config [KEY VALUE]  - Показать/изменить конфиг"
    }
}
'@

$LddnsScript | Out-File -FilePath "$InstallDir\lddns.ps1" -Encoding UTF8

@"
@echo off
powershell.exe -ExecutionPolicy Bypass -File "$InstallDir\lddns.ps1" %*
"@ | Out-File -FilePath "$InstallDir\lddns.bat" -Encoding ASCII

Write-Host ""
Write-Host "Установка завершена!" -ForegroundColor Green
Write-Host ""
Write-Host "Команды:"
Write-Host "  lddns enable          - Включить LDDNS"
Write-Host "  lddns disable         - Отключить LDDNS"
Write-Host "  lddns status          - Статус"
Write-Host "  lddns config          - Показать конфиг"
Write-Host ""
Write-Host "Примечание: Для Windows сервиса требуется NSSM (https://nssm.cc/)" -ForegroundColor Yellow
