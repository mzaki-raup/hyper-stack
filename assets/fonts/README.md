# Bundled fonts

These fonts are compiled into `HyperStack.exe` (`crates/hs-ui/src/fonts.rs`)
and offered in Settings → Appearance / Terminal / Editor next to the fonts
installed on the machine. Each one is redistributable under its own licence,
reproduced here verbatim as the licences require.

| Font | File | Licence | Role |
|---|---|---|---|
| JetBrainsMono Nerd Font Mono | `JetBrainsMonoNerdFontMono-Regular.ttf` | SIL OFL 1.1 (`JetBrainsMono-OFL.txt`) + Nerd Fonts patches, MIT (`NerdFonts-LICENSE.txt`) | Default terminal and editor font |
| Fira Code Nerd Font Mono | `FiraCodeNerdFontMono-Regular.ttf` | SIL OFL 1.1 (`FiraCode-OFL.txt`) + Nerd Fonts patches, MIT | Optional |
| BitstromWera Nerd Font Mono (Bitstream Vera Sans Mono) | `BitstromWeraNerdFontMono-Regular.ttf` | Bitstream Vera licence (`BitstreamVera-LICENSE.txt`, permissive, redistributable) + Nerd Fonts patches, MIT | Optional |

Not bundled: **Anthropic Sans** (proprietary brand font, not licensed for
redistribution). Like any other font installed on the machine it appears in
the system-font list and can be selected, but the app never ships it.

Source: the `patched-fonts/` tree of <https://github.com/ryanoasis/nerd-fonts>
(regular weight, "Mono" variant with the Nerd Font glyphs squeezed to one
cell). The OFL forbids selling the fonts on their own and requires derived
fonts to drop the reserved names; both are satisfied by embedding unchanged
files. Interface text uses egui's default fonts unless a system font is
chosen.
