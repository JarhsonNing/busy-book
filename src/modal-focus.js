// Focus stays inside the active modal and returns to its trigger on close.
const stack = [];
const focusable = (el) =>
  [
    ...el.querySelectorAll(
      'button:not(:disabled),summary,a[href],input:not(:disabled),select:not(:disabled),textarea:not(:disabled),[tabindex]:not([tabindex="-1"])',
    ),
  ].filter((n) => n.getClientRects().length);
export const modalFocus = {
  mounted(el, binding) {
    const previous = document.activeElement;
    const entry = { el, close: binding.value, previous };
    stack.push(entry);
    el.__modalEntry = entry;
    el.tabIndex = -1;
    entry.key = (e) => {
      if (stack.at(-1) !== entry) return;
      // Portalled selects own their first Escape/Tab; the modal resumes after close.
      if (el.querySelector('[role="combobox"][aria-expanded="true"]') ||
          e.target.closest?.('.select-popover')) return;
      if (e.key === "Escape" && typeof entry.close === "function") {
        e.preventDefault();
        e.stopPropagation();
        entry.close();
      }
      if (e.key === "Tab") {
        const items = focusable(el);
        const at = items.indexOf(document.activeElement);
        if (!items.length) {
          e.preventDefault();
          el.focus();
        } else if (e.shiftKey && at <= 0) {
          e.preventDefault();
          items.at(-1).focus();
        } else if (!e.shiftKey && (at < 0 || at === items.length - 1)) {
          e.preventDefault();
          items[0].focus();
        }
      }
    };
    document.addEventListener("keydown", entry.key, true);
    queueMicrotask(() => {
      if (el.isConnected) (focusable(el)[0] || el).focus();
    });
  },
  updated(el, binding) {
    if (el.__modalEntry) el.__modalEntry.close = binding.value;
  },
  unmounted(el) {
    const entry = el.__modalEntry;
    if (!entry) return;
    document.removeEventListener("keydown", entry.key, true);
    stack.splice(stack.indexOf(entry), 1);
    if (entry.previous?.isConnected) entry.previous.focus();
  },
};
