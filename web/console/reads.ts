// What the Actors page reads from the Console's own reads of a Session: the Now section,
// a Session's turn, and the rows that quote a transcript entry. Each answers from what the
// Console has read so far and is null while nothing has been read.
export interface ConsoleReads {
  recordingFor(sessionId: string): FeldConsoleRecording | null;
  runtimeRecordFor(sessionId: string, anchor: string, timeOrigin: FeldConsoleTimeOrigin | null): FeldConsoleRuntimeAnchor | null;
  lastRuntimeRecordFor(sessionId: string, timeOrigin: FeldConsoleTimeOrigin | null, live: boolean): FeldConsoleLastRecord | null;
  lastLogLineFor(sessionId: string, sessionStart: number, live: boolean): FeldConsoleLastRecord | null;
  feldRecordsFor(sessionId: string): FeldApi.FeldRecord[] | null;
  // The Session with what its own read found about its transcript file: the range reads
  // carry the state as of their read, and the Console's read is newer once it answers.
  consoleSessionFor(session: FeldConsoleSession): FeldConsoleSession;
}
