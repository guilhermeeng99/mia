# MIA — landing site

A tiny static page presenting the app and linking to downloads (GitHub Releases).
Stack: **Vite + Tailwind v4** (no framework). One page, no backend.

Standalone (not in the app's package manager workspace):

```bash
pnpm install   # run inside site/
pnpm dev                          # local preview
pnpm build                        # static output → dist/
```

Deploys to **GitHub Pages** automatically via `.github/workflows/deploy-site.yml` (or push
`dist/` to any static host). The download link in `index.html` points at GitHub Releases.

> The app itself uses Bun; this standalone site mirrors the sibling **Toolzy** site and uses
> pnpm so the deploy workflow stays identical. The favicon and header/footer mark
> are generated from `public/logo.png`.
