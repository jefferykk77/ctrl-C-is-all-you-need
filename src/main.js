const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let slotA = { hwnd: null, title: "" };
let slotB = { hwnd: null, title: "" };

window.addEventListener("DOMContentLoaded", async () => {
  // Window controls
  document.getElementById("btn-minimize").addEventListener("click", () => {
    invoke("minimize_window");
  });
  
  document.getElementById("btn-close").addEventListener("click", () => {
    invoke("close_window");
  });

  // Drag anywhere on the window (except buttons/cards) to move & bind
  const appWindow = document.querySelector(".app-window");
  appWindow.addEventListener("mousedown", async (e) => {
    // Prevent dragging when clicking buttons or interactive elements
    if (
      e.target.tagName === "BUTTON" || 
      e.target.closest("button") || 
      e.target.classList.contains("clear-slot-btn")
    ) {
      return;
    }
    
    // 1. Tell Rust backend that this drag is a target-binding drag
    await invoke("start_drag_detect");
    
    // 2. Start OS-native window dragging
    await invoke("start_window_drag");
  });

  // Listen to asynchronous target bindings from Rust backend subclassing
  await listen("slots-updated", async () => {
    await refreshSlots();
  });

  // Clear slot buttons
  document.querySelectorAll(".clear-slot-btn").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const slot = btn.getAttribute("data-slot");
      clearSlot(slot);
    });
  });

  // Sync initial slots state from backend
  await refreshSlots();
});

// Clears specific slot
async function clearSlot(slot) {
  if (slot === "A") {
    slotA = { hwnd: null, title: "" };
    await invoke("set_slot", { slot: "A", hwnd: null, title: null });
  } else {
    slotB = { hwnd: null, title: "" };
    await invoke("set_slot", { slot: "B", hwnd: null, title: null });
  }
  await refreshSlots();
}

// Refreshes UI cards and status indicators
async function refreshSlots() {
  const backendState = await invoke("get_slots");
  slotA = { hwnd: backendState.hwnd_a, title: backendState.title_a };
  slotB = { hwnd: backendState.hwnd_b, title: backendState.title_b };

  const cardA = document.getElementById("slot-a");
  const cardB = document.getElementById("slot-b");
  const statusIndicator = document.getElementById("status-indicator");
  const statusText = document.getElementById("status-text");

  // Render Slot A
  if (slotA.hwnd) {
    cardA.classList.remove("empty");
    cardA.classList.add("active-bound");
    cardA.querySelector(".slot-title").textContent = slotA.title;
  } else {
    cardA.classList.add("empty");
    cardA.classList.remove("active-bound");
    cardA.querySelector(".slot-title").textContent = "App to bind";
  }

  // Render Slot B
  if (slotB.hwnd) {
    cardB.classList.remove("empty");
    cardB.classList.add("active-bound");
    cardB.querySelector(".slot-title").textContent = slotB.title;
  } else {
    cardB.classList.add("empty");
    cardB.classList.remove("active-bound");
    cardB.querySelector(".slot-title").textContent = "App to bind";
  }

  // Render locked status channel
  if (slotA.hwnd && slotB.hwnd) {
    statusIndicator.classList.remove("inactive");
    statusIndicator.classList.add("active");
    statusText.textContent = "Locked CV Channel Active";
    await invoke("set_active", { active: true });
  } else {
    statusIndicator.classList.add("inactive");
    statusIndicator.classList.remove("active");
    statusText.textContent = "Bypass Mode";
    await invoke("set_active", { active: false });
  }
}
