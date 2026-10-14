# Janela do instalador macOS (DMG)

`background.svg` é o fundo da janela do DMG da Nova Foods: cor do app, logo Nova Foods (marca
registrada, termos em `docs/brand/LICENSE-brand.txt`), nome do app e a instrução de instalação. O
texto é Montserrat (OFL) já convertido em contornos, então nenhuma fonte fica no repositório.

`generate.py` desenha o `background.tiff` (1x e 2x) e o `dmg-layout.DS_Store` (tamanho da janela,
posição dos ícones) a partir dele:

    pip install 'pillow>=12' ds_store==1.3.3 mac_alias==2.2.3
    python3 packaging/macos/dmg/generate.py      # precisa do resvg no PATH

O app fica em (326, 205) e o atalho Aplicativos em (574, 205); a seta do fundo fica entre os dois.
