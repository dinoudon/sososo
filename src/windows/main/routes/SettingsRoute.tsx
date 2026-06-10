import { useEffect, useState } from 'react';
import { HugeiconsIcon } from '@hugeicons/react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { getVersion } from '@tauri-apps/api/app';
import {
  getAiModelSettings,
  getAiProvider,
  getSummaryLanguage,
  hasApiKey,
  listAiModels,
  listDevices,
  setAnthropicBaseUrl,
  setAnthropicModel,
  setAiProvider,
  setApiKey,
  setDevices,
  setGeminiModel,
  setOpenaiCompatibleBaseUrl,
  setOpenaiCompatibleModel,
  setOpenaiModel,
  setSummaryLanguage,
} from '../../../lib/ipc';
import { SUMMARY_LANGUAGES } from '../../../lib/languages';
import { isMacOS, isLinux } from '../../../lib/platform';
import {
  IconAlert,
  IconCheck,
  IconDevices,
  IconRegenerate,
  IconDownload,
  IconExternal,
  IconGift,
  IconKey,
  IconLanguage,
  IconMic,
  IconSpeaker,
} from '../../../lib/icons';
import { useConfigStore } from '../../../state/configStore';
import { useUpdateStore } from '../../../state/updateStore';
import { checkForUpdate, downloadAndInstall, restartApp } from '../../../lib/updater';
import type { AiProvider, ApiService, DeviceLists } from '../../../types/domain';
import { AppearanceSection } from './settings/AppearanceSection';
import { deriveUpdateStatus } from './settings/updateStatus';
import {
  BADGE_OPT,
  BADGE_REQ,
  BTN,
  BTN_PRIMARY,
  FIELD,
  FIELD_CTRL,
  FIELD_LABEL,
  H3,
  SUBHEAD,
} from './settings/styles';

// Open external links in the system browser (Tauri), with a plain-web fallback
// so the links still work under `vite dev` outside the Tauri webview.
async function openExternal(url: string) {
  try {
    await openUrl(url);
  } catch {
    window.open(url, '_blank', 'noopener,noreferrer');
  }
}

export default function SettingsRoute() {
  const [devices, setDeviceLists] = useState<DeviceLists | null>(null);
  const [dgKey, setDgKey] = useState('');
  const [oaKey, setOaKey] = useState('');
  const [gmKey, setGmKey] = useState('');
  const [ocKey, setOcKey] = useState(''); // openai-compatible (optional)
  const [anKey, setAnKey] = useState(''); // anthropic (optional)
  const [dgSaved, setDgSaved] = useState(false);
  const [oaSaved, setOaSaved] = useState(false);
  const [gmSaved, setGmSaved] = useState(false);
  const [ocSaved, setOcSaved] = useState(false);
  const [anSaved, setAnSaved] = useState(false);
  const [status, setStatus] = useState('');
  // AI-summary output language ("auto" or a language code), persisted in the DB.
  const [summaryLang, setSummaryLang] = useState('auto');
  // Active AI provider for summaries + live translation, persisted in the DB.
  const [aiProvider, setAiProviderState] = useState<AiProvider>('openai');
  // Provider-specific model / endpoint settings.
  const [openaiModel, setOpenaiModelState] = useState('gpt-4o-mini');
  const [geminiModel, setGeminiModelState] = useState('gemini-2.5-flash');
  const [ocBaseUrl, setOcBaseUrl] = useState('https://api.openai.com/v1');
  const [ocModel, setOcModel] = useState('gpt-4o-mini');
  const [anBaseUrl, setAnBaseUrl] = useState('https://api.anthropic.com');
  const [anModel, setAnModel] = useState('claude-sonnet-4-20250514');
  // Fetched model list for the active provider (null = not fetched yet).
  const [modelList, setModelList] = useState<string[] | null>(null);
  const [modelListLoading, setModelListLoading] = useState(false);

  // Device selection is shared with the Start-transcription screen via the config store.
  const inputDevice = useConfigStore((s) => s.inputDevice);
  const outputDevice = useConfigStore((s) => s.outputDevice);
  const setInputDevice = useConfigStore((s) => s.setInputDevice);
  const setOutputDevice = useConfigStore((s) => s.setOutputDevice);
  const autoSummarizeOnFinish = useConfigStore((s) => s.autoSummarizeOnFinish);
  const setAutoSummarizeOnFinish = useConfigStore((s) => s.setAutoSummarizeOnFinish);

  // App update (in-app updater): current version + the check/download/restart flow.
  const [appVersion, setAppVersion] = useState('');
  const updateStatus = useUpdateStore((s) => s.status);
  const updateVersion = useUpdateStore((s) => s.version);
  const updateDownloaded = useUpdateStore((s) => s.downloaded);
  const updateContentLength = useUpdateStore((s) => s.contentLength);
  const updateError = useUpdateStore((s) => s.error);

  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => {});
  }, []);

  useEffect(() => {
    listDevices()
      .then((d) => {
        setDeviceLists(d);
        // Seed defaults only if nothing is selected yet, so a choice made on
        // the Start-transcription screen isn't clobbered.
        const cfg = useConfigStore.getState();
        if (cfg.inputDevice == null) {
          setInputDevice(d.input.find((x) => x.isDefault)?.id ?? d.input[0]?.id ?? null);
        }
        if (cfg.outputDevice == null) {
          setOutputDevice(d.output.find((x) => x.isDefault)?.id ?? d.output[0]?.id ?? null);
        }
      })
      .catch((e) => setStatus(`Failed to load devices: ${e}`));
    hasApiKey('deepgram')
      .then(setDgSaved)
      .catch(() => {});
    hasApiKey('openai')
      .then(setOaSaved)
      .catch(() => {});
    hasApiKey('gemini')
      .then(setGmSaved)
      .catch(() => {});
    hasApiKey('openai-compatible')
      .then(setOcSaved)
      .catch(() => {});
    hasApiKey('anthropic')
      .then(setAnSaved)
      .catch(() => {});
    getSummaryLanguage()
      .then(setSummaryLang)
      .catch(() => {});
    getAiProvider()
      .then((p) => {
        setAiProviderState(p);
        void fetchModels();
      })
      .catch(() => {});
    getAiModelSettings()
      .then((s) => {
        setOpenaiModelState(s.openaiModel);
        setGeminiModelState(s.geminiModel);
        setOcBaseUrl(s.openaiCompatibleBaseUrl);
        setOcModel(s.openaiCompatibleModel);
        setAnBaseUrl(s.anthropicBaseUrl);
        setAnModel(s.anthropicModel);
      })
      .catch(() => {});
  }, [setInputDevice, setOutputDevice]);

  async function saveSummaryLanguage(code: string) {
    setSummaryLang(code);
    try {
      await setSummaryLanguage(code);
      setStatus('Summary language saved.');
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  }

  async function saveDevices() {
    try {
      await setDevices(inputDevice, outputDevice);
      setStatus('Devices saved.');
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  }

  async function fetchModels() {
    setModelListLoading(true);
    try {
      const list = await listAiModels();
      setModelList(list);
    } catch {
      setModelList([]); // empty = fetch failed; UI falls back to text input
    } finally {
      setModelListLoading(false);
    }
  }

  async function saveAiProvider(provider: AiProvider) {
    setAiProviderState(provider);
    setModelList(null); // clear stale list from the previous provider
    try {
      await setAiProvider(provider);
      setStatus('AI provider saved.');
      void fetchModels();
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  }

  async function saveKey(service: ApiService) {
    const keyMap: Record<ApiService, string> = {
      deepgram: dgKey,
      openai: oaKey,
      gemini: gmKey,
      'openai-compatible': ocKey,
      anthropic: anKey,
    };
    const value = keyMap[service].trim();
    if (!value) return;
    try {
      await setApiKey(service, value);
      const clearMap: Record<ApiService, () => void> = {
        deepgram: () => {
          setDgKey('');
          setDgSaved(true);
        },
        openai: () => {
          setOaKey('');
          setOaSaved(true);
        },
        gemini: () => {
          setGmKey('');
          setGmSaved(true);
        },
        'openai-compatible': () => {
          setOcKey('');
          setOcSaved(true);
        },
        anthropic: () => {
          setAnKey('');
          setAnSaved(true);
        },
      };
      clearMap[service]();
      setStatus(`${service} API key saved.`);
    } catch (e) {
      setStatus(`Error: ${e}`);
    }
  }

  // Derive the App-update status line from the updater store.
  const { msg: updateMsg, warn: updateMsgWarn } = deriveUpdateStatus(
    updateStatus,
    updateVersion,
    updateDownloaded,
    updateContentLength,
    updateError,
  );

  return (
    <div className="mx-auto max-w-[620px] px-8 py-7">
      <h2 className="mb-5 text-[20px] font-semibold">Settings</h2>

      <section className="mb-7">
        <h3 className={H3}>
          <HugeiconsIcon icon={IconKey} size={13} strokeWidth={1.8} aria-hidden={true} />
          API Keys
        </h3>
        {/* Many users confuse Deepgram with the AI keys and fill in only one,
            thinking they're alternatives. Make explicit that Deepgram is the
            required transcription engine and the AI keys are a separate optional add-on. */}
        <div className="mb-4 flex flex-col gap-1.5 rounded-sm border border-[rgba(255,255,255,0.14)] bg-[rgba(255,255,255,0.04)] px-3 py-2.5 text-[12px] leading-[1.55] text-fg-dim">
          <span>
            <b className="text-fg">Deepgram</b> and the <b className="text-fg">AI keys</b> (OpenAI /
            Gemini) are <b className="text-fg">two different services, not alternatives</b> — you
            don&apos;t choose one or the other.
          </span>
          <span>
            <b className="text-accent">Deepgram is required</b> — it powers live transcription, the
            app&apos;s core feature. The AI keys are <b className="text-fg">optional</b> and only
            add AI summaries &amp; live translation on top.
          </span>
        </div>
        {/* Group 1 — the required speech-to-text key */}
        <div className="mb-2 flex items-center gap-2">
          <span className={SUBHEAD}>Speech-to-text engine</span>
          <span className={BADGE_REQ}>Required</span>
        </div>
        <label className={FIELD}>
          <span className={FIELD_LABEL}>
            Deepgram API Key{' '}
            {dgSaved && (
              <em className="ml-1.5 inline-flex items-center gap-1 text-[11.5px] text-ok not-italic">
                <HugeiconsIcon icon={IconCheck} size={13} strokeWidth={2} aria-hidden={true} />
                saved
              </em>
            )}
          </span>
          <div className="flex gap-2">
            <input
              className={FIELD_CTRL}
              type="password"
              value={dgKey}
              onChange={(e) => setDgKey(e.target.value)}
              placeholder={dgSaved ? '••••••••••••' : 'Deepgram token…'}
            />
            <button className={BTN} onClick={() => void saveKey('deepgram')}>
              Save
            </button>
          </div>
        </label>
        {/* How to get a Deepgram key + the free-credit nudge */}
        <div className="-mt-1 mb-3.5 flex flex-col gap-1.5 rounded-sm border border-[rgba(110,168,254,0.28)] bg-[rgba(110,168,254,0.08)] px-3 py-2.5">
          <span className="inline-flex items-start gap-1.5 text-[12px] leading-[1.5] text-fg-dim">
            <HugeiconsIcon
              icon={IconGift}
              size={14}
              strokeWidth={1.8}
              className="mt-px shrink-0 text-accent"
              aria-hidden={true}
            />
            <span>
              New Deepgram accounts get <b className="text-fg">$200 in free credit</b> (~45,000
              minutes) — no credit card required.
            </span>
          </span>
          <button
            type="button"
            className="inline-flex w-fit cursor-pointer items-center gap-1.5 text-[12.5px] text-accent hover:underline"
            onClick={() => void openExternal('https://console.deepgram.com/signup')}
          >
            Get a free Deepgram API key
            <HugeiconsIcon icon={IconExternal} size={13} strokeWidth={1.8} aria-hidden={true} />
          </button>
          <span className="text-[11px] leading-[1.5] text-fg-faint">
            Sign up, create an API key in the Deepgram console, then paste it above.
          </span>
        </div>
        {/* Group 2 — the optional AI keys (summaries + live translation) */}
        <div className="mt-5 mb-2 flex items-center gap-2">
          <span className={SUBHEAD}>AI summaries &amp; translation</span>
          <span className={BADGE_OPT}>Optional</span>
        </div>
        <p className="mb-3 text-[11.5px] leading-[1.5] text-fg-faint">
          These are separate from Deepgram. Skip them entirely if you only need transcription —
          recording and live captions work with the Deepgram key alone.
        </p>
        <label className={FIELD}>
          <span className={FIELD_LABEL}>
            OpenAI API Key{' '}
            {oaSaved && (
              <em className="ml-1.5 inline-flex items-center gap-1 text-[11.5px] text-ok not-italic">
                <HugeiconsIcon icon={IconCheck} size={13} strokeWidth={2} aria-hidden={true} />
                saved
              </em>
            )}
          </span>
          <div className="flex gap-2">
            <input
              className={FIELD_CTRL}
              type="password"
              value={oaKey}
              onChange={(e) => setOaKey(e.target.value)}
              placeholder={oaSaved ? '••••••••••••' : 'sk-…'}
            />
            <button className={BTN} onClick={() => void saveKey('openai')}>
              Save
            </button>
          </div>
        </label>
        <button
          type="button"
          className="mb-1 inline-flex w-fit cursor-pointer items-center gap-1.5 text-[12px] text-fg-dim hover:text-accent hover:underline"
          onClick={() => void openExternal('https://platform.openai.com/api-keys')}
        >
          Get an OpenAI API key (optional — for AI summaries)
          <HugeiconsIcon icon={IconExternal} size={12} strokeWidth={1.8} aria-hidden={true} />
        </button>
        <label className={FIELD}>
          <span className={FIELD_LABEL}>
            Gemini API Key{' '}
            {gmSaved && (
              <em className="ml-1.5 inline-flex items-center gap-1 text-[11.5px] text-ok not-italic">
                <HugeiconsIcon icon={IconCheck} size={13} strokeWidth={2} aria-hidden={true} />
                saved
              </em>
            )}
          </span>
          <div className="flex gap-2">
            <input
              className={FIELD_CTRL}
              type="password"
              value={gmKey}
              onChange={(e) => setGmKey(e.target.value)}
              placeholder={gmSaved ? '••••••••••••' : 'AIza…'}
            />
            <button className={BTN} onClick={() => void saveKey('gemini')}>
              Save
            </button>
          </div>
        </label>
        <button
          type="button"
          className="mb-1 inline-flex w-fit cursor-pointer items-center gap-1.5 text-[12px] text-fg-dim hover:text-accent hover:underline"
          onClick={() => void openExternal('https://aistudio.google.com/app/apikey')}
        >
          Get a Gemini API key (optional — for AI summaries)
          <HugeiconsIcon icon={IconExternal} size={12} strokeWidth={1.8} aria-hidden={true} />
        </button>
        <label className={FIELD}>
          <span className={FIELD_LABEL}>Active AI provider</span>
          <select
            className={FIELD_CTRL}
            value={aiProvider}
            onChange={(e) => void saveAiProvider(e.target.value as AiProvider)}
          >
            <option value="openai">OpenAI</option>
            <option value="openai-compatible">OpenAI Compatible (custom endpoint)</option>
            <option value="gemini">Gemini</option>
            <option value="anthropic">Anthropic</option>
          </select>
          <span className="text-[11.5px] leading-[1.4] text-fg-faint">
            Powers AI session summaries and live translation. Set the matching API key above.
          </span>
        </label>

        {/* Provider-specific model / endpoint settings */}
        {(aiProvider === 'openai' ||
          aiProvider === 'openai-compatible' ||
          aiProvider === 'gemini' ||
          aiProvider === 'anthropic') && (
          <div className={FIELD}>
            <span className={FIELD_LABEL}>
              Model
              {modelListLoading && (
                <span className="ml-2 text-[11px] text-fg-faint">Fetching…</span>
              )}
              {!modelListLoading && modelList !== null && modelList.length > 0 && (
                <span className="ml-2 text-[11px] text-fg-faint">{modelList.length} available</span>
              )}
            </span>
            <div className="flex gap-2">
              {modelList && modelList.length > 0 ? (
                <select
                  className={FIELD_CTRL}
                  value={
                    aiProvider === 'openai'
                      ? openaiModel
                      : aiProvider === 'gemini'
                        ? geminiModel
                        : aiProvider === 'openai-compatible'
                          ? ocModel
                          : anModel
                  }
                  onChange={(e) => {
                    const v = e.target.value;
                    if (aiProvider === 'openai') setOpenaiModelState(v);
                    else if (aiProvider === 'gemini') setGeminiModelState(v);
                    else if (aiProvider === 'openai-compatible') setOcModel(v);
                    else setAnModel(v);
                  }}
                >
                  {(() => {
                    const current =
                      aiProvider === 'openai'
                        ? openaiModel
                        : aiProvider === 'gemini'
                          ? geminiModel
                          : aiProvider === 'openai-compatible'
                            ? ocModel
                            : anModel;
                    return (
                      <>
                        {current && !modelList.includes(current) && (
                          <option value={current}>{current} (custom)</option>
                        )}
                        {modelList.map((m) => (
                          <option key={m} value={m}>
                            {m}
                          </option>
                        ))}
                      </>
                    );
                  })()}
                </select>
              ) : (
                <input
                  className={FIELD_CTRL}
                  value={
                    aiProvider === 'openai'
                      ? openaiModel
                      : aiProvider === 'gemini'
                        ? geminiModel
                        : aiProvider === 'openai-compatible'
                          ? ocModel
                          : anModel
                  }
                  onChange={(e) => {
                    const v = e.target.value;
                    if (aiProvider === 'openai') setOpenaiModelState(v);
                    else if (aiProvider === 'gemini') setGeminiModelState(v);
                    else if (aiProvider === 'openai-compatible') setOcModel(v);
                    else setAnModel(v);
                  }}
                  placeholder={
                    aiProvider === 'openai'
                      ? 'gpt-4o-mini'
                      : aiProvider === 'gemini'
                        ? 'gemini-2.5-flash'
                        : aiProvider === 'openai-compatible'
                          ? 'gpt-4o-mini'
                          : 'claude-sonnet-4-20250514'
                  }
                />
              )}
              <button
                className={BTN}
                onClick={() => {
                  const save =
                    aiProvider === 'openai'
                      ? setOpenaiModel(openaiModel)
                      : aiProvider === 'gemini'
                        ? setGeminiModel(geminiModel)
                        : aiProvider === 'openai-compatible'
                          ? setOpenaiCompatibleModel(ocModel)
                          : setAnthropicModel(anModel);
                  void save
                    .then(() => setStatus('Model saved.'))
                    .catch((e: unknown) => setStatus(`Error: ${e}`));
                }}
              >
                Save
              </button>
              <button
                className={BTN}
                disabled={modelListLoading}
                onClick={() => void fetchModels()}
                title="Refresh model list from provider"
              >
                <HugeiconsIcon icon={IconRegenerate} size={13} strokeWidth={2} aria-hidden={true} />
              </button>
            </div>
            {!modelListLoading && modelList !== null && modelList.length === 0 && (
              <span className="text-[11px] text-fg-faint">
                Could not fetch model list — enter a model name manually, or check your API key and
                base URL.
              </span>
            )}
          </div>
        )}

        {aiProvider === 'openai-compatible' && (
          <>
            <label className={FIELD}>
              <span className={FIELD_LABEL}>Base URL</span>
              <div className="flex gap-2">
                <input
                  className={FIELD_CTRL}
                  value={ocBaseUrl}
                  onChange={(e) => setOcBaseUrl(e.target.value)}
                  placeholder="https://api.openai.com/v1"
                />
                <button
                  className={BTN}
                  onClick={() => {
                    void setOpenaiCompatibleBaseUrl(ocBaseUrl)
                      .then(() => {
                        setStatus('Base URL saved.');
                        void fetchModels();
                      })
                      .catch((e: unknown) => setStatus(`Error: ${e}`));
                  }}
                >
                  Save
                </button>
              </div>
              <span className="text-[11.5px] leading-[1.4] text-fg-faint">
                Any endpoint that speaks the OpenAI Chat Completions API — Ollama, LM Studio, local
                models.
              </span>
            </label>
            <label className={FIELD}>
              <span className={FIELD_LABEL}>
                API Key (optional){' '}
                {ocSaved && (
                  <em className="ml-1.5 inline-flex items-center gap-1 text-[11.5px] text-ok not-italic">
                    <HugeiconsIcon icon={IconCheck} size={13} strokeWidth={2} aria-hidden={true} />
                    saved
                  </em>
                )}
              </span>
              <div className="flex gap-2">
                <input
                  className={FIELD_CTRL}
                  type="password"
                  value={ocKey}
                  onChange={(e) => setOcKey(e.target.value)}
                  placeholder={ocSaved ? '••••••••••••' : 'Leave blank for local endpoints…'}
                />
                <button className={BTN} onClick={() => void saveKey('openai-compatible')}>
                  Save
                </button>
              </div>
            </label>
          </>
        )}

        {aiProvider === 'anthropic' && (
          <>
            <label className={FIELD}>
              <span className={FIELD_LABEL}>Base URL</span>
              <div className="flex gap-2">
                <input
                  className={FIELD_CTRL}
                  value={anBaseUrl}
                  onChange={(e) => setAnBaseUrl(e.target.value)}
                  placeholder="https://api.anthropic.com"
                />
                <button
                  className={BTN}
                  onClick={() => {
                    void setAnthropicBaseUrl(anBaseUrl)
                      .then(() => {
                        setStatus('Base URL saved.');
                        void fetchModels();
                      })
                      .catch((e: unknown) => setStatus(`Error: ${e}`));
                  }}
                >
                  Save
                </button>
              </div>
              <span className="text-[11.5px] leading-[1.4] text-fg-faint">
                Leave as default for Anthropic's cloud API, or point to an Anthropic-compatible
                local endpoint.
              </span>
            </label>
            <label className={FIELD}>
              <span className={FIELD_LABEL}>
                API Key (optional){' '}
                {anSaved && (
                  <em className="ml-1.5 inline-flex items-center gap-1 text-[11.5px] text-ok not-italic">
                    <HugeiconsIcon icon={IconCheck} size={13} strokeWidth={2} aria-hidden={true} />
                    saved
                  </em>
                )}
              </span>
              <div className="flex gap-2">
                <input
                  className={FIELD_CTRL}
                  type="password"
                  value={anKey}
                  onChange={(e) => setAnKey(e.target.value)}
                  placeholder={anSaved ? '••••••••••••' : 'sk-ant-…'}
                />
                <button className={BTN} onClick={() => void saveKey('anthropic')}>
                  Save
                </button>
              </div>
            </label>
          </>
        )}
        <label className="mb-3.5 flex cursor-pointer items-start gap-2.5">
          <input
            type="checkbox"
            className="mt-0.5 h-4 w-4 shrink-0 cursor-pointer accent-[#6ea8fe]"
            checked={autoSummarizeOnFinish}
            onChange={(e) => setAutoSummarizeOnFinish(e.target.checked)}
          />
          <span className="flex flex-col gap-0.5">
            <span className={FIELD_LABEL}>Auto-summarize when a recording finishes</span>
            <span className="text-[11.5px] leading-[1.4] text-fg-faint">
              Generates the AI summary automatically as soon as a recording ends, using the active
              provider above. Needs that provider&apos;s API key — otherwise it&apos;s skipped.
            </span>
          </span>
        </label>
        <p className="mt-2 text-[12px] leading-[1.5] text-fg-faint">
          Keys are stored securely in{' '}
          {isMacOS
            ? 'the macOS Keychain'
            : isLinux
              ? 'the system keyring (GNOME Keyring / KWallet)'
              : 'Windows Credential Manager'}{' '}
          and are never sent to the frontend.
        </p>
      </section>

      <section className="mb-7">
        <h3 className={H3}>
          <HugeiconsIcon icon={IconDevices} size={13} strokeWidth={1.8} aria-hidden={true} />
          Audio Devices
        </h3>
        <label className={FIELD}>
          <span className={`${FIELD_LABEL} inline-flex items-center gap-1.5`}>
            <HugeiconsIcon icon={IconMic} size={13} strokeWidth={1.8} aria-hidden={true} />
            Microphone
          </span>
          <select
            className={FIELD_CTRL}
            value={inputDevice ?? ''}
            onChange={(e) => setInputDevice(e.target.value || null)}
          >
            {devices?.input.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
                {d.isDefault ? ' (default)' : ''}
              </option>
            ))}
          </select>
        </label>
        <label className={FIELD}>
          <span className={`${FIELD_LABEL} inline-flex items-center gap-1.5`}>
            <HugeiconsIcon icon={IconSpeaker} size={13} strokeWidth={1.8} aria-hidden={true} />
            {isMacOS || isLinux
              ? 'System audio source'
              : 'System audio source (the output to loop back)'}
          </span>
          <select
            className={FIELD_CTRL}
            value={outputDevice ?? ''}
            onChange={(e) => setOutputDevice(e.target.value || null)}
          >
            {devices?.output.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
                {d.isDefault ? ' (default)' : ''}
              </option>
            ))}
          </select>
          {isMacOS && (
            <span className="text-[11.5px] leading-[1.4] text-fg-faint">
              macOS can&apos;t capture speakers directly. Install a virtual audio device like{' '}
              <b className="text-fg-dim">BlackHole</b>, route your output into it via a Multi-Output
              Device, then select it here.
            </span>
          )}
          {isLinux && (
            <span className="text-[11.5px] leading-[1.4] text-fg-faint">
              System audio is captured automatically from your default output&apos;s{' '}
              <b className="text-fg-dim">monitor</b> (PulseAudio/PipeWire) — no setup needed. Pick a
              specific output here to capture that one instead.
            </span>
          )}
        </label>
        <button
          className="mt-1 cursor-pointer rounded-sm border border-[rgba(255,255,255,0.3)] bg-[rgba(110,168,254,0.24)] px-4 py-[9px] text-[13px] whitespace-nowrap text-[#dbe8ff] shadow-liquid hover:bg-[rgba(110,168,254,0.34)]"
          onClick={() => void saveDevices()}
        >
          Save devices
        </button>
      </section>

      <AppearanceSection />

      <section className="mb-7">
        <h3 className={H3}>
          <HugeiconsIcon icon={IconLanguage} size={13} strokeWidth={1.8} aria-hidden={true} />
          Language
        </h3>
        <label className={FIELD}>
          <span className={FIELD_LABEL}>AI summary language</span>
          <select
            className={FIELD_CTRL}
            value={summaryLang}
            onChange={(e) => void saveSummaryLanguage(e.target.value)}
          >
            {SUMMARY_LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.label}
              </option>
            ))}
          </select>
          <span className="text-[11.5px] leading-[1.4] text-fg-faint">
            Language used for AI session summaries. "Auto" matches the transcript language. Saved to
            the database.
          </span>
        </label>
        <p className="mt-2 text-[12px] leading-[1.5] text-fg-faint">
          All languages now use Deepgram's <b>Nova-3</b> model (best accuracy, including
          Indonesian). Pick the language on the main screen before you start recording. The{' '}
          <i>Auto-detect (multilingual)</i> option recognizes a mix of languages automatically, but
          choosing one specific language is usually more accurate.
        </p>
      </section>

      <section className="mb-7">
        <h3 className={H3}>
          <HugeiconsIcon icon={IconDownload} size={13} strokeWidth={1.8} aria-hidden={true} />
          App update
        </h3>
        <p className="mb-3 text-[12px] leading-[1.5] text-fg-faint">
          Updates install in-app from GitHub Releases — no need to download installers manually.
          {appVersion && (
            <>
              {' '}
              You&apos;re on <b className="text-fg-dim">version {appVersion}</b>.
            </>
          )}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            className={`${BTN} disabled:cursor-default disabled:opacity-60`}
            disabled={updateStatus === 'checking' || updateStatus === 'downloading'}
            onClick={() => void checkForUpdate()}
          >
            {updateStatus === 'checking' ? 'Checking…' : 'Check for updates'}
          </button>
          {updateStatus === 'available' && (
            <button type="button" className={BTN_PRIMARY} onClick={() => void downloadAndInstall()}>
              Download &amp; install{updateVersion ? ` ${updateVersion}` : ''}
            </button>
          )}
          {updateStatus === 'ready' && (
            <button type="button" className={BTN_PRIMARY} onClick={() => void restartApp()}>
              Restart to update
            </button>
          )}
        </div>
        {updateMsg && (
          <p
            className={`mt-2.5 text-[12.5px] leading-[1.4] ${
              updateMsgWarn ? 'text-[#ffb454]' : 'text-fg-dim'
            }`}
          >
            {updateMsg}
          </p>
        )}
      </section>

      {status && (
        <p
          className={`mt-2 inline-flex items-center gap-1.5 text-[12.5px] ${
            /^(Error|Failed)/.test(status) ? 'text-[#ffb454]' : 'text-ok'
          }`}
        >
          <HugeiconsIcon
            icon={/^(Error|Failed)/.test(status) ? IconAlert : IconCheck}
            size={14}
            strokeWidth={1.8}
            aria-hidden={true}
          />
          {status}
        </p>
      )}
    </div>
  );
}
