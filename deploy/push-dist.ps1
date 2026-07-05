# Полный цикл обновления браузерного клиента: trunk build → заливка на VPS →
# выкладка в /opt/cs2d/dist (строго заменяет содержимое, старые файлы удаляются).
# Запуск из любого места: .\deploy\push-dist.ps1
# Параметры по умолчанию — боевой сервер; переопределяются: -HostName x -Port 22
param(
    [string]$HostName = "46.173.28.234",
    [int]$Port = 2222,
    [string]$User = "deploy"
)
$ErrorActionPreference = "Stop"
$repo = Split-Path $PSScriptRoot -Parent
$target = "$User@$HostName"

Write-Host "== trunk build =="
Push-Location (Join-Path $repo "client")
try { trunk build; if ($LASTEXITCODE -ne 0) { throw "trunk build failed" } }
finally { Pop-Location }

Write-Host "== upload dist =="
# ЛОВУШКА scp: в существующий каталог он кладёт копию ВНУТРЬ (~/dist/dist).
# Поэтому цель сносится заранее — папка на VPS всегда строго новая.
ssh -p $Port $target "rm -rf ~/dist"
if ($LASTEXITCODE -ne 0) { throw "ssh rm failed" }
scp -P $Port -r (Join-Path $repo "dist") "${target}:~/dist"
if ($LASTEXITCODE -ne 0) { throw "scp failed" }

Write-Host "== deploy to /opt/cs2d/dist =="
ssh -t -p $Port $target "sudo rsync -a --delete ~/dist/ /opt/cs2d/dist/ && echo '--- deployed: ---' && ls /opt/cs2d/dist | grep client-"
if ($LASTEXITCODE -ne 0) { throw "remote deploy failed" }

Write-Host "== done: закрой игровые вкладки и открой сайт заново (F5) =="
