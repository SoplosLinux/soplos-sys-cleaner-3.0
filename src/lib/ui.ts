import { writable } from "svelte/store";

// Whether the explanatory panels (concept, what is kept, equivalent command)
// are shown next to the lists. Off on every launch; the user turns them on
// with the Notes button in the hero.
export const showNotes = writable<boolean>(false);

// Number of dialogs open right now. Keyboard shortcuts stay quiet while one is
// open, so nothing quits, rescans or selects behind a confirmation.
export const modalDepth = writable(0);

// True when the key press happens in a place where typing is expected (text
// fields, the language selector). Checkboxes and buttons do not count.
export function isTypingTarget(event: KeyboardEvent): boolean {
  const element = event.target as HTMLElement | null;
  if (!element) return false;
  if (element.isContentEditable) return true;
  if (element.tagName === "TEXTAREA" || element.tagName === "SELECT") return true;
  if (element.tagName === "INPUT") {
    return !["checkbox", "radio", "button", "submit"].includes((element as HTMLInputElement).type);
  }
  return false;
}

export const toastMessage = writable<string | null>(null);

let toastTimer: ReturnType<typeof setTimeout> | undefined;

export function toast(message: string) {
  toastMessage.set(message);
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastMessage.set(null), 2600);
}
