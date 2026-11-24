"use strict";

const providerSearch = document.querySelector("#provider-search");
providerSearch?.addEventListener("input", () => {
  const q = providerSearch.value.trim().toLowerCase();
  document.querySelectorAll("[data-provider]").forEach((el) => {
    el.hidden = !el.dataset.provider.toLowerCase().includes(q);
  });
});

// Collapse the provider sidebar by default on narrow mobile screens so the
// main search bar and model grid remain immediately visible.
const providerPanel = document.querySelector("#provider-panel");
if (providerPanel && window.matchMedia("(max-width: 820px)").matches) {
  providerPanel.open = false;
}

const dialog = document.querySelector("#sdk-dialog");
document.querySelector("#sdk-open")?.addEventListener("click", () => dialog?.showModal());
document.querySelector("#sdk-close")?.addEventListener("click", () => dialog?.close());
dialog?.addEventListener("click", (event) => {
  if (event.target === dialog) {
    dialog.close();
  }
});

document.addEventListener("click", async (event) => {
  const button = event.target.closest("[data-copy],[data-copy-target]");
  if (!button) return;
  const text = button.dataset.copy ?? document.getElementById(button.dataset.copyTarget)?.textContent;
  if (text === undefined) return;
  const original = button.textContent;
  try {
    await navigator.clipboard.writeText(text);
    button.textContent = "已复制";
  } catch {
    button.textContent = "复制失败";
  }
  setTimeout(() => {
    button.textContent = original;
  }, 1400);
});
