<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { i18n } from "../i18n";

  const win = getCurrentWindow();

  let maximized = $state(false);

  async function refreshMaximized() {
    maximized = await win.isMaximized().catch(() => false);
  }

  async function minimize() {
    await win.minimize();
  }

  async function toggleMaximize() {
    await win.toggleMaximize();
    await refreshMaximized();
  }

  async function close() {
    await win.close();
  }

  function stopControlEvent(event: MouseEvent) {
    event.stopPropagation();
  }

  $effect(() => {
    refreshMaximized();
  });
</script>

<header
  class="window-titlebar"
  role="presentation"
  data-tauri-drag-region
  ondblclick={toggleMaximize}
>
  <div class="window-title" data-tauri-drag-region>
    <img src="/logo.png" alt="" class="window-title-logo" data-tauri-drag-region />
    <span data-tauri-drag-region>MIA</span>
  </div>

  <div class="window-controls" role="group" aria-label={$i18n.window.controls}>
    <button
      type="button"
      class="window-control"
      aria-label={$i18n.window.minimize}
      onmousedown={stopControlEvent}
      ondblclick={stopControlEvent}
      onclick={minimize}
    >
      <span class="window-glyph minimize"></span>
    </button>
    <button
      type="button"
      class="window-control"
      aria-label={maximized ? $i18n.window.restore : $i18n.window.maximize}
      onmousedown={stopControlEvent}
      ondblclick={stopControlEvent}
      onclick={toggleMaximize}
    >
      <span class:maximized class="window-glyph maximize"></span>
    </button>
    <button
      type="button"
      class="window-control close"
      aria-label={$i18n.window.close}
      onmousedown={stopControlEvent}
      ondblclick={stopControlEvent}
      onclick={close}
    >
      <span class="window-glyph close-x"></span>
    </button>
  </div>
</header>
<div class="titlebar-divider"></div>

<style>
  .titlebar-divider {
    height: 2px;
    flex-shrink: 0;
    background: var(--color-charcoal);
  }

  .window-titlebar {
    display: flex;
    height: 34px;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    background: var(--color-canvas-deep);
    color: var(--color-ink-soft);
    cursor: default;
    user-select: none;
  }

  .window-title {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    font-family: var(--font-body);
    font-size: 12px;
    font-weight: 800;
    line-height: 1;
  }

  .window-title-logo {
    width: 18px;
    height: 18px;
    border-radius: 5px;
  }

  .window-controls {
    display: flex;
    height: 100%;
    align-items: stretch;
  }

  .window-control {
    position: relative;
    display: grid;
    width: 44px;
    height: 34px;
    place-items: center;
    border: 0;
    background: transparent;
    color: #6b625e;
    padding: 0;
  }

  .window-control:hover {
    background: rgba(0, 0, 0, 0.06);
    color: #201916;
  }

  .window-control.close:hover {
    background: #e75b4c;
    color: white;
  }

  .window-glyph {
    position: relative;
    display: block;
    width: 12px;
    height: 12px;
  }

  .window-glyph.minimize::before {
    content: "";
    position: absolute;
    left: 1px;
    right: 1px;
    top: 7px;
    height: 1.5px;
    border-radius: 999px;
    background: currentColor;
  }

  .window-glyph.maximize::before {
    content: "";
    position: absolute;
    inset: 1px;
    border: 2px solid currentColor;
    border-radius: 3px;
  }

  .window-glyph.maximize.maximized::before {
    inset: 3px 1px 1px 3px;
  }

  .window-glyph.maximize.maximized::after {
    content: "";
    position: absolute;
    inset: 1px 3px 3px 1px;
    border: 2px solid currentColor;
    border-radius: 3px;
    background: var(--color-canvas-deep);
  }

  .window-control:hover .window-glyph.maximize.maximized::after {
    background: color-mix(in srgb, var(--color-canvas-deep) 94%, black);
  }

  .window-glyph.close-x::before,
  .window-glyph.close-x::after {
    content: "";
    position: absolute;
    left: 1px;
    right: 1px;
    top: 5px;
    height: 1.5px;
    border-radius: 999px;
    background: currentColor;
  }

  .window-glyph.close-x::before {
    transform: rotate(45deg);
  }

  .window-glyph.close-x::after {
    transform: rotate(-45deg);
  }
</style>
