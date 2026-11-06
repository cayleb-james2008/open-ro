import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

describe('Keyboard-First Workflows & Accessible Focus Suite', () => {
  const app = readFileSync('src/App.tsx', 'utf8').replace(/\r\n/g, '\n');
  const styles = readFileSync('src/styles.css', 'utf8').replace(/\r\n/g, '\n');

  describe('Keyboard Shortcuts & Event Handlers in App.tsx', () => {
    it('binds global keydown listener with clean teardown', () => {
      expect(app).toContain("window.addEventListener('keydown', handleGlobalKeydown);");
      expect(app).toContain("return () => window.removeEventListener('keydown', handleGlobalKeydown);");
    });

    it('implements Ctrl+K / Cmd+K composer focus shortcut', () => {
      expect(app).toContain("if (mod && (event.key === 'k' || event.key === 'K')) {");
      expect(app).toContain("const input = document.getElementById('composer-input');");
      expect(app).toContain('input?.focus();');
    });

    it('implements Escape shortcut to stop running turn or dismiss modal', () => {
      expect(app).toContain("if (event.key === 'Escape') {");
      expect(app).toContain('setShortcutsOpen(false);');
      expect(app).toContain('void stopTurn();');
    });

    it('implements Ctrl+Shift+N / Cmd+Shift+N new chat shortcut', () => {
      expect(app).toContain("if (mod && event.shiftKey && (event.key === 'n' || event.key === 'N')) {");
      expect(app).toContain('newChat();');
    });

    it('implements Ctrl+O / Cmd+O project selection shortcut', () => {
      expect(app).toContain("if (mod && (event.key === 'o' || event.key === 'O')) {");
      expect(app).toContain('void selectProject();');
    });

    it('implements Ctrl+1..4 view switching shortcuts', () => {
      expect(app).toContain("if (mod && event.key === '1') {");
      expect(app).toContain("setScreen('chat');");
      expect(app).toContain("if (mod && event.key === '2') {");
      expect(app).toContain("setScreen('models');");
      expect(app).toContain("if (mod && event.key === '3') {");
      expect(app).toContain("setScreen('workspace');");
      expect(app).toContain("if (mod && event.key === '4') {");
      expect(app).toContain("setScreen('integrations');");
    });

    it('implements Alt+A and Alt+R quick approval resolution hotkeys', () => {
      expect(app).toContain("if (event.altKey && (event.key === 'a' || event.key === 'A') && !inInput) {");
      expect(app).toContain("void resolveApproval(approvals[0].id, 'approve');");
      expect(app).toContain("if (event.altKey && (event.key === 'r' || event.key === 'R') && !inInput) {");
      expect(app).toContain("void resolveApproval(approvals[0].id, 'reject');");
    });

    it('implements ? and Ctrl+/ shortcuts guide toggles', () => {
      expect(app).toContain("if (event.key === '?' && !inInput && !mod && !event.altKey) {");
      expect(app).toContain('setShortcutsOpen(true);');
      expect(app).toContain("if (mod && event.key === '/') {");
      expect(app).toContain('setShortcutsOpen((prev) => !prev);');
    });

    it('declares aria-keyshortcuts on the composer textarea', () => {
      expect(app).toContain('aria-keyshortcuts="Control+Enter Meta+Enter Control+K Meta+K"');
    });

    it('renders keyboard shortcuts dialog modal with table', () => {
      expect(app).toContain('<Dialog labelledBy="shortcuts-title" onClose={() => setShortcutsOpen(false)}>');
      expect(app).toContain('<strong id="shortcuts-title">Keyboard Shortcuts</strong>');
      expect(app).toContain('<table className="shortcuts-table" aria-label="Available keyboard shortcuts">');
    });
  });

  describe('Focus & Accessibility Styles in styles.css', () => {
    it('defines prominent :focus-visible ring with accent color', () => {
      expect(styles).toContain(':focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; border-radius: 4px; }');
      expect(styles).toContain('button:focus-visible, input:focus-visible, textarea:focus-visible, [tabindex="0"]:focus-visible, a:focus-visible');
    });

    it('defines accessible keyboard badge and table styles', () => {
      expect(styles).toContain('kbd {');
      expect(styles).toContain('font-family: var(--font-mono);');
      expect(styles).toContain('color: var(--color-text);');
      expect(styles).toContain('background: var(--color-raised);');
      expect(styles).toContain('.shortcuts-table {');
      expect(styles).toContain('.shortcut-hint {');
    });

    it('provides accessible skip-link for keyboard screen readers', () => {
      expect(styles).toContain('.skip-link {');
      expect(styles).toContain('.skip-link:focus {');
    });
  });
});
