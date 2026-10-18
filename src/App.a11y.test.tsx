// @vitest-environment jsdom
/**
 * Whole-app tests in a simulated browser: accessibility (axe), keyboard safety of the
 * confirmation screen, and the offline request path. The Tauri backend is mocked.
 */
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

const backend: Record<string, unknown> = {
  get_app_status: {
    appVersion: "0.2.0",
    os: "windows",
    speechRecognition: "needs_setup",
    speechSynthesis: "available",
    reasoningModel: "needs_setup",
    computerControl: "limited",
  },
  get_preferences: { textSize: "normal", speakReplies: false, practiceMode: false, fileSearch: false },
  set_preferences: { textSize: "normal", speakReplies: false, practiceMode: false, fileSearch: false },
  get_speech_setup: { configured: false, region: null },
  get_assistant_setup: { configured: false, model: "gemini-3.8-flash" },
  get_diagnostics: "",
};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd in backend) return backend[cmd];
    if (cmd === "log_frontend_error") return null;
    throw `unexpected command ${cmd}`;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import App from "./App";
import { ConfirmDialog } from "./ui/ConfirmDialog";

beforeAll(() => {
  // jsdom doesn't implement modal dialogs.
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.removeAttribute("open");
  };
});

afterEach(cleanup);

async function expectNoAxeViolations(node: Element) {
  const results = await axe.run(node, {
    // jsdom has no layout, so colour contrast can't be measured here.
    rules: { "color-contrast": { enabled: false } },
  });
  const summary = results.violations.map((v) => `${v.id}: ${v.help} (${v.nodes.length})`);
  expect(summary).toEqual([]);
}

async function renderApp() {
  const utils = render(<App />);
  await screen.findByText("Ready");
  return utils;
}

describe("accessibility", () => {
  it("home screen has no axe violations", async () => {
    const { container } = await renderApp();
    await expectNoAxeViolations(container);
  });

  it("settings screen has no axe violations", async () => {
    const { container } = await renderApp();
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    await screen.findByText("What this version can do");
    await expectNoAxeViolations(container);
  });

  it("help screen has no axe violations", async () => {
    const { container } = await renderApp();
    await userEvent.click(screen.getByRole("button", { name: "Help" }));
    await screen.findByText("Something not working?");
    await expectNoAxeViolations(container);
  });

  it("every control can be reached by name (no unlabelled buttons or fields)", async () => {
    await renderApp();
    for (const b of screen.getAllByRole("button")) {
      expect((b.textContent ?? "").trim() || b.getAttribute("aria-label")).toBeTruthy();
    }
    expect(screen.getByRole("textbox", { name: "Type a request" })).toBeTruthy();
  });

  it("the current page is announced in the navigation", async () => {
    await renderApp();
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(within(nav).getByRole("button", { name: "Home" }).getAttribute("aria-current")).toBe("page");
  });
});

describe("confirmation screen", () => {
  const request = {
    id: "c1",
    action: "Send a message (practice: nothing will really be sent)",
    details: [
      { label: "To", value: "Anna" },
      { label: "Message", value: "See you at five." },
    ],
    practice: true,
    expiresInSecs: 120,
  };

  it("has no axe violations and starts on the safe choice", async () => {
    const { container } = render(
      <ConfirmDialog request={request} onAnswer={() => {}} listening={false} hint={null} />,
    );
    await expectNoAxeViolations(container);
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "No" }));
  });

  it("Enter on the initially focused button means No, never Yes", async () => {
    const onAnswer = vi.fn();
    render(<ConfirmDialog request={request} onAnswer={onAnswer} listening={false} hint={null} />);
    await userEvent.keyboard("{Enter}");
    expect(onAnswer).toHaveBeenCalledWith(false);
    expect(onAnswer).not.toHaveBeenCalledWith(true);
  });

  it("Yes requires choosing the Yes button explicitly", async () => {
    const onAnswer = vi.fn();
    render(<ConfirmDialog request={request} onAnswer={onAnswer} listening={false} hint={null} />);
    await userEvent.click(screen.getByRole("button", { name: "Yes, do it" }));
    expect(onAnswer).toHaveBeenCalledWith(true);
  });

  it("shows the exact details it was given", () => {
    render(<ConfirmDialog request={request} onAnswer={() => {}} listening={false} hint={null} />);
    expect(screen.getByText("Anna")).toBeTruthy();
    expect(screen.getByText("See you at five.")).toBeTruthy();
    expect(screen.getByText(/Practice: nothing will really happen/)).toBeTruthy();
  });
});

describe("offline request path (no assistant key)", () => {
  it("a typed request gets an honest reply in the conversation", async () => {
    await renderApp();
    await userEvent.type(screen.getByRole("textbox", { name: "Type a request" }), "hello there");
    await act(async () => {
      await userEvent.click(screen.getByRole("button", { name: "Send" }));
    });
    expect(await screen.findByText(/I can only do a few things so far/)).toBeTruthy();
  });

  it("Stop is always present and disabled when there's nothing to stop", async () => {
    await renderApp();
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(stop.hasAttribute("disabled")).toBe(true);
  });
});
