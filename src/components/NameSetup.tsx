import { useState, type FormEvent } from 'react';
import { buttonPrimary, inputBase } from '../ui/ui';

export const DISPLAY_NAME_STORAGE_KEY = 'securechat:displayName';

/** 读取持久化的显示名；localStorage 不可用时返回空串（仅本次会话有效）。 */
export function loadDisplayName(): string {
  try {
    return window.localStorage.getItem(DISPLAY_NAME_STORAGE_KEY) ?? '';
  } catch {
    return '';
  }
}

/** 持久化显示名；失败时静默降级（不影响本次会话使用）。 */
export function saveDisplayName(name: string): void {
  try {
    window.localStorage.setItem(DISPLAY_NAME_STORAGE_KEY, name);
  } catch {
    /* webview localStorage 不可用 */
  }
}

interface NameSetupProps {
  onSave: (name: string) => void;
}

/**
 * 首次启动的显示名设置引导（issue #7）：
 * 没有保存过名称时覆盖在主界面上，设置一次后记住，后续启动直接使用。
 */
export function NameSetup({ onSave }: NameSetupProps) {
  const [name, setName] = useState('');

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) return;
    saveDisplayName(trimmed);
    onSave(trimmed);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-bg/80 p-4 backdrop-blur">
      <form
        className="w-full max-w-sm rounded-2xl border border-line bg-surface p-6 shadow-xl"
        onSubmit={submit}
        aria-labelledby="name-setup-title"
      >
        <h2 id="name-setup-title" className="m-0 text-base font-semibold tracking-tight text-ink">
          设置你的显示名
        </h2>
        <p className="mt-1.5 text-xs leading-normal text-ink-muted">
          对方在消息列表中会看到这个名字（名字只经加密通道传输）。设置一次后自动记住，下次启动直接使用。
        </p>
        <label htmlFor="name-setup-input" className="sr-only">
          显示名
        </label>
        <input
          id="name-setup-input"
          name="displayName"
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="输入显示名"
          autoComplete="nickname"
          maxLength={32}
          autoFocus
          required
          className={`${inputBase} mt-4`}
        />
        <button type="submit" className={`${buttonPrimary} mt-4 w-full`} disabled={!name.trim()}>
          保存并继续
        </button>
      </form>
    </div>
  );
}
