/**
 * Chat sessions are scoped to the project that created them. Sessions without
 * a projectRoot (legacy chats, or chats started before a project was opened)
 * stay visible everywhere so shipping this feature never hides history.
 */
export interface SessionProjectRef {
  projectRoot?: string;
}

/** Compare project roots the way a desktop app should: slashes and case ignored, no trailing separator. */
export function normalizeProjectPath(value: string): string {
  return value.trim().replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();
}

/**
 * Visible when no project is open (the full history stays reachable), or the
 * session has no project, or it belongs to the open project.
 */
export function isSessionVisibleForProject(session: SessionProjectRef, activeProjectRoot: string | null): boolean {
  if (!activeProjectRoot) {
    return true;
  }
  if (!session.projectRoot) {
    return true;
  }
  return normalizeProjectPath(session.projectRoot) === normalizeProjectPath(activeProjectRoot);
}
