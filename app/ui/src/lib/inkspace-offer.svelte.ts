/**
 * The first-run wizard's offer to import Wacom Inkspace's notes (`Wizard.svelte`): when the user
 * continues with the box checked, `pending` is set, and the library screen that follows opens the
 * import dialog once (`Home.svelte`). The user still confirms the import there.
 */
export const inkspaceOffer = $state({ pending: false });
