/** Turns SDK and browser error text into something a non-technical user can act on. */
export function friendlyRecognitionError(details: string): string {
  if (/NotAllowedError|Permission denied|permission/i.test(details)) {
    return "I'm not allowed to use the microphone. Please allow microphone access for this app in Windows Settings, under Privacy & security, Microphone.";
  }
  if (/NotFoundError|Requested device not found|no audio|microphone initialization/i.test(details)) {
    return "I couldn't find a microphone. Please check one is plugged in and turned on.";
  }
  if (/401|403|Unauthorized|authentication/i.test(details)) {
    return "Microsoft didn't accept the speech key. Please check it in Settings.";
  }
  if (/1006|websocket|network|connection|timeout/i.test(details)) {
    return "I couldn't reach Microsoft's speech service. Please check the internet connection.";
  }
  return "Something went wrong while listening. Please try again.";
}
