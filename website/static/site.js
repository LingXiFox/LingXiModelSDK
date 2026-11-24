"use strict";
const input = document.querySelector("#provider-search");
input?.addEventListener("input", () => {
  const q = input.value.trim().toLowerCase();
  document.querySelectorAll("[data-provider]").forEach(el => {
    el.hidden = !el.dataset.provider.toLowerCase().includes(q);
  });
});
const dialog = document.querySelector("#sdk-dialog");
document.querySelector("#sdk-open")?.addEventListener("click", () => dialog.showModal());
document.querySelector("#sdk-close")?.addEventListener("click", () => dialog.close());
document.addEventListener("click", async event => {
  const button = event.target.closest("[data-copy],[data-copy-target]");
  if (!button) return;
  const text = button.dataset.copy ?? document.getElementById(button.dataset.copyTarget)?.textContent;
  if (text === undefined) return;
  const original = button.textContent;
  try { await navigator.clipboard.writeText(text); button.textContent = "已复制"; }
  catch { button.textContent = "复制失败"; }
  setTimeout(() => { button.textContent = original; }, 1400);
});
