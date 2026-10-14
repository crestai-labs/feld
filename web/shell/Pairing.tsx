import { useEffect, useRef, useState, type FormEvent } from "react";
import { browserAccess, normalizeCode, pairingClient, signIn, type PairingFailure, type PairingNotice, type PairingResult } from "../api/auth.ts";
import { usePreferences } from "./preferences.ts";
import { t } from "./i18n.ts";

// The characters a code is written in: digits and capitals without 0, 1, I, L, and O.
const CODE_ALPHABET = /^[2-9A-HJKMNP-Z]*$/;
const CODE_LENGTH = 8;
const COPIED_DURATION = 1400;
// How long "Signed in" shows before the page opens.
const OPEN_DELAY = 600;
// The queries the Actors page keeps when the pairing page leads to it.
const PAGE_QUERY_KEYS = ["workspace", "scenario"];
const PAIR_COMMAND = "feld ui pair";

function formatted(code: string) {
  return code.length > 4 ? `${code.slice(0, 4)}-${code.slice(4)}` : code;
}

// The pairing page (`/ui/pair`) leads to the Actors page; any other page shows itself
// again, now with the cookie and the key.
function destination(pairPage: boolean) {
  const url = new URL(location.href);
  url.hash = "";
  url.searchParams.delete("state");
  if (!pairPage) return url.href;
  const next = new URL("/ui/", location.href);
  for (const key of PAGE_QUERY_KEYS) {
    const value = url.searchParams.get(key);
    if (value !== null) next.searchParams.set(key, value);
  }
  return next.href;
}

function CopyCommand() {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), COPIED_DURATION);
    return () => window.clearTimeout(timer);
  }, [copied]);
  return (
    <button
      type="button"
      className={`feld-pairing__copy${copied ? " is-copied" : ""}`}
      data-pairing-action="copy"
      aria-label={`${t("feld.copy")}: ${PAIR_COMMAND}`}
      title={t(copied ? "feld.copied" : "feld.copy")}
      onClick={() => {
        const done = () => setCopied(true);
        if (navigator.clipboard?.writeText) navigator.clipboard.writeText(PAIR_COMMAND).then(done, done);
        else done();
      }}
    >
      <svg viewBox="0 0 16 16" aria-hidden="true"><rect x="5.5" y="5.5" width="8" height="8" rx="1.5"></rect><path d="M10.5 3.5v-.5A1.5 1.5 0 0 0 9 1.5H3A1.5 1.5 0 0 0 1.5 3v6A1.5 1.5 0 0 0 3 10.5h.5"></path></svg>
    </button>
  );
}

// The pairing screen (design/auth.md#pairing), which the person sees as Sign in, in place of
// the page's sidebar and content.
// The person enters a code `feld ui pair` issued on the daemon's machine, and the daemon
// answers with the browser token's cookie and key. `feld ui` opens `/ui/pair#code=<code>`, whose
// code is sent without typing. A fixture passes its own request (fixtures/index.ts).
export function Pairing({ notice, pairPage, send }: { notice: PairingNotice | null; pairPage: boolean; send?: (code: string) => Promise<boolean> }) {
  usePreferences((state) => state.revision);
  const [pair] = useState(() => pairingClient(send));
  const [code, setCode] = useState("");
  const [pending, setPending] = useState(false);
  const [result, setResult] = useState<PairingResult | null>(null);
  const [automatic, setAutomatic] = useState(() => Boolean(new URLSearchParams(location.hash.slice(1)).get("code")));
  // A sign-in that ended with neither a browser token nor a 401 (an absent daemon, a
  // refused Origin, a check of the browser token that failed).
  const [failure, setFailure] = useState<PairingFailure | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const fragmentRead = useRef(false);

  const canSubmit = (candidate: string) => !pending && result !== "paired" && candidate.length === CODE_LENGTH && CODE_ALPHABET.test(candidate);

  async function submit(candidate: string, automaticSubmission = false) {
    setPending(true);
    setResult(null);
    setFailure(null);
    const outcome = await signIn(pair, candidate, send ? undefined : browserAccess);
    const failed = typeof outcome === "string" ? null : outcome;
    const answer = failed ? null : outcome as PairingResult;
    // The daemon took the code, so the field cannot send it again.
    const codeConsumed = answer === "cookieRejected" || failed?.kind === "unconfirmed";
    setPending(false);
    setResult(answer);
    setFailure(failed);
    if (answer === "paired") {
      const delay = automaticSubmission ? 0 : OPEN_DELAY;
      window.setTimeout(() => location.replace(destination(pairPage)), delay);
    } else {
      if (codeConsumed) setCode("");
      setAutomatic(false);
      window.setTimeout(() => input.current?.select());
    }
  }

  // `feld ui` opens /ui/pair#code=<code>. The code leaves the address bar at once, so it
  // stays out of the history, and goes to the daemon as if typed; a code out of shape stays
  // in the field with the charset hint.
  useEffect(() => {
    if (fragmentRead.current) return;
    fragmentRead.current = true;
    const fragment = new URLSearchParams(location.hash.slice(1)).get("code");
    if (!fragment) return;
    history.replaceState(null, "", `${location.pathname}${location.search}`);
    const next = normalizeCode(fragment).slice(0, CODE_LENGTH);
    setCode(next);
    if (!canSubmit(next)) {
      setAutomatic(false);
      return;
    }
    void (async () => {
      try {
        if (!send && await browserAccess()) {
          location.replace(destination(pairPage));
          return;
        }
      } catch {
        // Let the pairing request report the reachable daemon's error, if any.
      }
      await submit(next, true);
    })();
    // Only the fragment the page opened with.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (canSubmit(code)) void submit(code);
  }

  function message() {
    if (pending) return <p className="feld-pairing__message" role="status">{t("pairing.submitting")}</p>;
    if (result === "paired") return <p className="feld-pairing__message is-ok" role="status">{t("pairing.paired")}</p>;
    if (result) return <p className="feld-pairing__message is-error" role="alert">{t(`pairing.${result}`)}</p>;
    if (failure) {
      return (
        <p className="feld-pairing__message is-error" role="alert">
          {t(`pairing.${failure.kind}`, { message: failure.message })}
          {/* The pairing page reads no browser token of its own; the page it leads to reads
              with it, or gives way to Sign in without one. */}
          {failure.kind === "unconfirmed" ? <> <a href={destination(pairPage)}>{t("pairing.open")}</a></> : null}
        </p>
      );
    }
    if (!CODE_ALPHABET.test(code)) return <p className="feld-pairing__message is-error" role="alert">{t("pairing.charset")}</p>;
    return <p className="feld-pairing__message">{t("pairing.charset")}</p>;
  }

  const locked = pending || result === "paired";
  if (automatic) {
    return (
      <feld-pairing>
        <main className="feld-pairing" aria-label={t("pairing.opening")}>
          <div className="feld-pairing__loading" role="status">
            <span className="sidebar__feld-logo" aria-hidden="true"></span>
          </div>
        </main>
      </feld-pairing>
    );
  }
  return (
    <feld-pairing>
      <main className="feld-pairing" aria-labelledby="feld-pairing-title">
        <div className="feld-pairing__card">
          <header className="feld-pairing__brand">
            <span className="sidebar__feld-logo" aria-hidden="true"></span>
            <strong>Feld</strong>
          </header>
          {notice ? <p className="feld-pairing__notice" role="status">{t(`pairing.notice.${notice}`)}</p> : null}
          <h1 id="feld-pairing-title">{t("pairing.title")}</h1>
          <p className="feld-pairing__intro">{t("pairing.intro")}</p>
          <ol className="feld-pairing__steps">
            <li>
              <span>{t("pairing.step.issue")}</span>
              <span className="feld-pairing__command"><code>{PAIR_COMMAND}</code>
                <CopyCommand />
              </span>
              <small>{t("pairing.step.name")}</small>
            </li>
            <li><span>{t("pairing.step.enter")}</span></li>
          </ol>
          <form className="feld-pairing__form" noValidate onSubmit={onSubmit}>
            <label htmlFor="feld-pairing-code">{t("pairing.code.label")}</label>
            <div className="feld-pairing__field">
              <input
                id="feld-pairing-code" name="code" ref={input} value={formatted(code)} placeholder="XXXX-XXXX" maxLength={9}
                autoComplete="one-time-code" autoCapitalize="characters" autoCorrect="off" spellCheck={false} inputMode="text"
                aria-describedby="feld-pairing-message" disabled={locked}
                onChange={(event) => {
                  // The field keeps what was typed as the code reads: capitals, a hyphen after four.
                  setCode(normalizeCode(event.target.value).slice(0, CODE_LENGTH));
                  setResult(null);
                  setFailure(null);
                }}
              />
              <button type="submit" className="feld-pairing__submit" disabled={!canSubmit(code)}>{t("pairing.submit")}</button>
            </div>
            <div id="feld-pairing-message">{message()}</div>
          </form>
          <p className="feld-pairing__local">{t("pairing.local")}</p>
        </div>
      </main>
    </feld-pairing>
  );
}
