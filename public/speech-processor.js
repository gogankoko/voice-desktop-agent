// Microphone AudioWorklet for the Azure Speech SDK. Identical to the processor the SDK
// would otherwise build from a blob: URL, which the app's Content Security Policy blocks.
// Served from the app itself so `script-src 'self'` allows it.
class SP extends AudioWorkletProcessor {
  process(inputs) {
    const input = inputs[0];
    if (input && input[0]) this.port.postMessage(input[0]);
    return true;
  }
}
registerProcessor("speech-processor", SP);
