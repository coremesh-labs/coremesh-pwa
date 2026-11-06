# Kleiner statischer Server für web\ (Entwicklung und Test), Port 8080.
# Aufruf:  powershell -ExecutionPolicy Bypass -File scripts\serve.ps1
param([int]$Port = 8080)

$root = Join-Path (Split-Path -Parent $PSScriptRoot) 'web'
$mime = @{
    '.html' = 'text/html; charset=utf-8'; '.js' = 'text/javascript; charset=utf-8'; '.mjs' = 'text/javascript; charset=utf-8'
    '.css' = 'text/css; charset=utf-8'; '.json' = 'application/json; charset=utf-8'; '.wasm' = 'application/wasm'
    '.webmanifest' = 'application/manifest+json'; '.svg' = 'image/svg+xml'; '.txt' = 'text/plain; charset=utf-8'
    '.pdf' = 'application/pdf'; '.png' = 'image/png'; '.jpg' = 'image/jpeg'
}

$listener = [System.Net.HttpListener]::new()
$listener.Prefixes.Add("http://localhost:$Port/")
$listener.Start()
Write-Host "Serving $root on http://localhost:$Port/"

while ($listener.IsListening) {
    $ctx = $listener.GetContext()
    try {
        $rel = [System.Uri]::UnescapeDataString($ctx.Request.Url.AbsolutePath.TrimStart('/'))
        if ($rel -eq '' -or $rel.EndsWith('/')) { $rel += 'index.html' }
        $path = [System.IO.Path]::GetFullPath((Join-Path $root $rel))
        if (-not $path.StartsWith([System.IO.Path]::GetFullPath($root)) -or -not (Test-Path $path -PathType Leaf)) {
            $ctx.Response.StatusCode = 404
            $bytes = [System.Text.Encoding]::UTF8.GetBytes('nicht gefunden')
        } else {
            $ext = [System.IO.Path]::GetExtension($path).ToLower()
            $ctx.Response.ContentType = if ($mime.ContainsKey($ext)) { $mime[$ext] } else { 'application/octet-stream' }
            $ctx.Response.Headers['Cache-Control'] = 'no-cache'
            $bytes = [System.IO.File]::ReadAllBytes($path)
        }
        $ctx.Response.ContentLength64 = $bytes.Length
        $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
    } catch {
        $ctx.Response.StatusCode = 500
    } finally {
        $ctx.Response.OutputStream.Close()
    }
}
