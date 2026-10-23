import { ref } from 'vue';

const toasts = ref([]);
let seq = 0;
const timers = new Map();

function remove(id) {
  toasts.value = toasts.value.filter((item) => item.id !== id);
  const timer = timers.get(id);
  if (timer) {
    clearTimeout(timer);
    timers.delete(id);
  }
}

function push(message, type = 'info', ttl = 4200) {
  const id = ++seq;
  toasts.value.push({ id, message, type });
  timers.set(id, setTimeout(() => remove(id), ttl));
  return id;
}

export function useToast() {
  return {
    toasts,
    remove,
    push,
    success: (message) => push(message, 'success'),
    error: (message) => push(message, 'error', 6000),
    info: (message) => push(message, 'info'),
  };
}
