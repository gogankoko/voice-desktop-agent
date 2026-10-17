import * as sdk from "microsoft-cognitiveservices-speech-sdk";
import { getSpeechToken } from "../ipc/speech";
import { friendlyRecognitionError } from "./errors";
import type { SpeechRecognizer } from "./types";

// Generous pauses: people often stop mid-sentence to think.
const INITIAL_SILENCE_MS = "8000";
const END_SILENCE_MS = "1500";

export class AzureRecognizer implements SpeechRecognizer {
  private recognizer: sdk.SpeechRecognizer | null = null;
  private settle: ((text: string) => void) | null = null;

  async listenOnce(onInterim: (text: string) => void): Promise<string> {
    this.cancel();
    // Rust returns a short-lived token; the resource key never reaches this code.
    const { token, region } = await getSpeechToken();

    const config = sdk.SpeechConfig.fromAuthorizationToken(token, region);
    config.speechRecognitionLanguage = "en-US";
    config.setProperty(sdk.PropertyId.SpeechServiceConnection_InitialSilenceTimeoutMs, INITIAL_SILENCE_MS);
    config.setProperty(sdk.PropertyId.SpeechServiceConnection_EndSilenceTimeoutMs, END_SILENCE_MS);
    const recognizer = new sdk.SpeechRecognizer(config, sdk.AudioConfig.fromDefaultMicrophoneInput());
    this.recognizer = recognizer;
    recognizer.recognizing = (_s, e) => onInterim(e.result.text);

    return new Promise<string>((resolve, reject) => {
      let done = false;
      const finish = (fn: () => void) => {
        if (done) return;
        done = true;
        this.settle = null;
        this.release(recognizer);
        fn();
      };
      this.settle = (text) => finish(() => resolve(text));

      recognizer.recognizeOnceAsync(
        (result) => {
          if (result.reason === sdk.ResultReason.RecognizedSpeech) {
            finish(() => resolve(result.text.trim()));
          } else if (result.reason === sdk.ResultReason.Canceled) {
            const details = sdk.CancellationDetails.fromResult(result);
            if (details.reason === sdk.CancellationReason.EndOfStream) finish(() => resolve(""));
            else finish(() => reject(new Error(friendlyRecognitionError(details.errorDetails ?? ""))));
          } else {
            finish(() => resolve("")); // NoMatch: speech we couldn't make out, or silence
          }
        },
        (err) => finish(() => reject(new Error(friendlyRecognitionError(String(err))))),
      );
    });
  }

  cancel(): void {
    const settle = this.settle;
    if (settle) settle("");
    else if (this.recognizer) this.release(this.recognizer);
  }

  private release(recognizer: sdk.SpeechRecognizer) {
    if (this.recognizer === recognizer) this.recognizer = null;
    try {
      recognizer.close(); // stops audio capture and closes the connection
    } catch {
      // already closed
    }
  }
}
