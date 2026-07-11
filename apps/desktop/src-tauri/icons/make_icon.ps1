$source = Join-Path $PSScriptRoot 'icon.png'
$destination = Join-Path $PSScriptRoot 'icon.ico'
$png = [System.IO.File]::ReadAllBytes($source)
$stream = [System.IO.File]::Create($destination)
$writer = [System.IO.BinaryWriter]::new($stream)

try {
    # ICO header and one 256x256 PNG-backed directory entry.
    $writer.Write([UInt16]0)
    $writer.Write([UInt16]1)
    $writer.Write([UInt16]1)
    $writer.Write([Byte]0)
    $writer.Write([Byte]0)
    $writer.Write([Byte]0)
    $writer.Write([Byte]0)
    $writer.Write([UInt16]1)
    $writer.Write([UInt16]32)
    $writer.Write([UInt32]$png.Length)
    $writer.Write([UInt32]22)
    $writer.Write($png)
}
finally {
    $writer.Dispose()
}
