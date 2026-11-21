import { buttonVariants } from "./button-variants";

// Keys in the compact toolbar above a preview (workspace files, attachments): 32px on
// desktop, 44px on phones. Neither key takes a size that also sets a minimum height,
// because the larger of the two heights would win over the one set here.
export const toolbarIconKey = `${buttonVariants({ variant: "ghost", size: "iconSm" })} max-sm:h-11 max-sm:w-11 max-sm:pointer-coarse:h-[44px] max-sm:pointer-coarse:w-[44px]`;

export const toolbarTextKey = `${buttonVariants({ variant: "ghost", size: null })} h-8 px-2 text-xs max-sm:h-11 max-sm:pointer-coarse:h-[44px]`;

// Keys in the title bar of the preview dialog: 40px on desktop, 44px on phones, with 18px icons.
export const dialogIconKey = `${buttonVariants({ variant: "ghost", size: "icon" })} max-sm:h-11 max-sm:w-11 max-sm:pointer-coarse:h-[44px] max-sm:pointer-coarse:w-[44px]`;
