$xavier = "E:\cortex\xavier\target\release\xavier.exe"
$memoryDir = "C:\Users\belal\clawd\agents\lasantacruz\memory"

# Index each memory file into Xavier
$files = Get-ChildItem $memoryDir -Filter "*.md" | Sort-Object Name

foreach ($f in $files) {
    $c = Get-Content $f.FullName -Raw -ErrorAction SilentlyContinue
    if ($c.Length -gt 0) {
        $d = $c.Substring(0, [Math]::Min(200, $c.Length))
        $d = $d.Replace('"', "'").Replace("`n", " ").Replace("`r", " ")
        $baseName = $f.BaseName
        & $xavier add "memory $baseName $d" "$baseName" 2>&1 | Out-Null
        Write-Host "Indexed memory: $baseName - $($c.Length) chars"
    }
}

Write-Host "Done indexing $($files.Count) memory files!"
