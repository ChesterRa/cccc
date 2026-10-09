// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { VoiceAudioSettings } from "../voice/VoiceAudioSettings";
import { useVoiceAudioStore } from "../../stores/useVoiceAudioStore";

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});
vi.mock("../../components/SelectCombobox", () => ({
  SelectCombobox: ({
    items,
    value,
    onChange,
    ariaLabel,
    placeholder,
  }: {
    items: { value: string; label: string }[];
    value: string;
    onChange: (value: string) => void;
    ariaLabel: string;
    placeholder: string;
  }) => (
    <select aria-label={ariaLabel} value={value} onChange={(event) => onChange(event.target.value)}>
      {!items.some((item) => item.value === value) && <option value={value}>{placeholder}</option>}
      {items.map((item) => (
        <option key={item.value} value={item.value}>
          {item.label}
        </option>
      ))}
    </select>
  ),
}));

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const enumerateDevices = vi.fn();
const getUserMedia = vi.fn();

const start = vi.fn();
const outputDescriptor = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, "setSinkId");
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.stubGlobal("navigator", { mediaDevices: { enumerateDevices, getUserMedia } });
  Object.defineProperty(HTMLMediaElement.prototype, "setSinkId", {
    configurable: true,
    value: vi.fn(),
  });
  enumerateDevices.mockReset();
  getUserMedia.mockReset();

  start.mockReset();
  useVoiceAudioStore.setState({
    preferences: { inputDeviceId: "saved-mic", outputDeviceId: "saved-speaker" },
    storageError: false,
  });
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  if (outputDescriptor)
    Object.defineProperty(HTMLMediaElement.prototype, "setSinkId", outputDescriptor);
  else Reflect.deleteProperty(HTMLMediaElement.prototype, "setSinkId");
});
const render = (active = true) =>
  act(async () => root.render(<VoiceAudioSettings active={active} />));
const select = (name: string) =>
  host.querySelector<HTMLSelectElement>(`select[aria-label="${name}"]`)!;
const refresh = () => act(async () => host.querySelector<HTMLButtonElement>("button")!.click());

it("preserves unavailable device choices and only changes preferences after an explicit selection", async () => {
  enumerateDevices.mockResolvedValue([
    { kind: "audioinput", deviceId: "", label: "" },
    { kind: "audiooutput", deviceId: "", label: "" },
    { kind: "audioinput", deviceId: "new-mic", label: "Fixture microphone" },
  ]);
  await render(false);
  expect(enumerateDevices).not.toHaveBeenCalled();
  await render();
  const microphone = select("voiceAudio.microphone");
  expect(microphone.value).toBe("saved-mic");
  expect(microphone.selectedOptions[0].textContent).toBe("voiceAudio.savedUnavailable");
  expect(microphone.querySelectorAll('option[value=""]')).toHaveLength(1);
  expect(select("voiceAudio.speaker").value).toBe("saved-speaker");
  expect(useVoiceAudioStore.getState().preferences).toEqual({
    inputDeviceId: "saved-mic",
    outputDeviceId: "saved-speaker",
  });
  await act(async () => {
    microphone.value = "new-mic";
    microphone.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(useVoiceAudioStore.getState().preferences).toEqual({
    inputDeviceId: "new-mic",
    outputDeviceId: "saved-speaker",
  });
  expect(getUserMedia).not.toHaveBeenCalled();
  expect(start).not.toHaveBeenCalled();
});

it("recovers device enumeration without requesting microphone access or replacing saved preferences", async () => {
  enumerateDevices.mockRejectedValueOnce(new Error("Fixture device enumeration failed"));
  await render();
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("voiceAudio.unavailable");
  enumerateDevices.mockResolvedValueOnce([
    { kind: "audioinput", deviceId: "saved-mic", label: "Fixture mic" },
  ]);
  await refresh();
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(select("voiceAudio.microphone").selectedOptions[0].textContent).toBe("Fixture mic");
  expect(useVoiceAudioStore.getState().preferences).toEqual({
    inputDeviceId: "saved-mic",
    outputDeviceId: "saved-speaker",
  });
  expect(getUserMedia).not.toHaveBeenCalled();
  expect(start).not.toHaveBeenCalled();
});

it("uses the system speaker on unsupported browsers without changing the saved output", async () => {
  Reflect.deleteProperty(HTMLMediaElement.prototype, "setSinkId");
  enumerateDevices.mockResolvedValue([]);
  await render();
  expect(select("voiceAudio.speaker")).toBeNull();
  expect(host.textContent).toContain("voiceAudio.systemSpeaker");
  expect(useVoiceAudioStore.getState().preferences).toEqual({
    inputDeviceId: "saved-mic",
    outputDeviceId: "saved-speaker",
  });
  expect(getUserMedia).not.toHaveBeenCalled();
});

it("reports unavailable storage while retaining the device choice for this page", async () => {
  enumerateDevices.mockResolvedValue([]);
  await render();
  await act(async () => useVoiceAudioStore.setState({ storageError: true }));
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("voiceAudio.storageUnavailable");
  expect(select("voiceAudio.microphone").value).toBe("saved-mic");
  expect(getUserMedia).not.toHaveBeenCalled();
});
