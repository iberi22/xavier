$xavier = "E:\cortex\xavier\target\release\xavier.exe"
$skillsDir = "C:\Users\belal\clawd\agents\lasantacruz\skills"

$skills = @("contenido-manager","cortex-memory","minimax-tools","negocio-contabilidad","porn-compilations","redes-despliegue","remotion-vertical-tutorials","tiktok-live-clips","video-generation")

foreach ($s in $skills) {
    $path = Join-Path $skillsDir ($s + "\SKILL.md")
    if (Test-Path $path) {
        $c = Get-Content $path -Raw -ErrorAction SilentlyContinue
        if ($c.Length -gt 0) {
            $d = $c.Substring(0, [Math]::Min(300, $c.Length))
            $d = $d.Replace('"', "'").Replace("`n", " ").Replace("`r", " ")
            $t = "skill " + $s
            & $xavier add "$t $d" "$t" 2>&1 | Out-Null
            Write-Host "Indexed skill: $s - $($c.Length) chars"
        }
    }
}

Write-Host "Done indexing skills!"
