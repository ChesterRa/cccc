import type { LucideIcon, LucideProps } from "lucide-react";
import {
  AppWindow,
  Archive,
  ArchiveRestore,
  ArrowDown,
  AudioLines,
  Bookmark,
  Bell,
  Camera,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronsUpDown,
  CircleAlert,
  CircleUserRound,
  Clock3,
  Check,
  Clipboard,
  ClipboardPaste,
  Compass,
  Copy,
  EllipsisVertical,
  Ellipsis,
  File,
  FileCheck,
  FileText,
  Folder,
  Globe,
  GripVertical,
  Headphones,
  House,
  Image,
  Inbox,
  Info,
  LayoutGrid,
  LayoutPanelLeft,
  Link2,
  LockKeyhole,
  Mail,
  Maximize2,
  Menu,
  Mic,
  MicOff,
  MessageSquare,
  MessageSquareQuote,
  MessageSquareText,
  Minimize2,
  Monitor,
  Moon,
  Paperclip,
  Pause,
  Pencil,
  Play,
  Plus,
  Power,
  RefreshCw,
  Reply,
  Rocket,
  Search,
  Send,
  Settings2,
  Sparkles,
  Square,
  SquareTerminal,
  Sun,
  TextCursorInput,
  Trash2,
  Undo2,
  Download,
  Volume2,
  X,
} from "lucide-react";
import { LcdGlyph, type LcdGlyphName } from "./LcdGlyph";

type IconProps = Omit<LucideProps, "ref">;

// Adding a glyph as a child would otherwise change lucide's default aria-hidden.
// Preserve its existing behavior, including explicit labels, roles and titles.
function hasAccessibleProps(props: IconProps) {
  return Object.keys(props).some(
    (key) => key.startsWith("aria-") || key === "role" || key === "title",
  );
}

function renderIcon(Icon: LucideIcon, glyph: LcdGlyphName | undefined, props: IconProps) {
  // Custom children retain their original SVG behavior and are never hidden by
  // the LCD glyph switch.
  if (!glyph || props.children != null) return <Icon {...props} />;
  const ariaHidden = hasAccessibleProps(props) ? undefined : true;
  return (
    <Icon aria-hidden={ariaHidden} {...props} data-lcd-icon="">
      <LcdGlyph name={glyph} />
    </Icon>
  );
}

function createIcon(Icon: LucideIcon, defaultStrokeWidth = 1.5, glyph?: LcdGlyphName) {
  function WrappedIcon({
    size = 18,
    strokeWidth = defaultStrokeWidth,
    absoluteStrokeWidth = true,
    ...props
  }: IconProps) {
    return renderIcon(Icon, glyph, { size, strokeWidth, absoluteStrokeWidth, ...props });
  }

  WrappedIcon.displayName = `Wrapped${Icon.displayName || "Icon"}`;
  return WrappedIcon;
}

function createControlIcon(Icon: LucideIcon, defaultStrokeWidth = 1.75, glyph?: LcdGlyphName) {
  function WrappedIcon({
    size = 18,
    strokeWidth = defaultStrokeWidth,
    absoluteStrokeWidth = true,
    ...props
  }: IconProps) {
    return renderIcon(Icon, glyph, {
      size,
      strokeWidth,
      absoluteStrokeWidth,
      fill: "none",
      ...props,
    });
  }

  WrappedIcon.displayName = `Control${Icon.displayName || "Icon"}`;
  return WrappedIcon;
}

export const ClipboardIcon = createIcon(Clipboard, 1.5, "clipboard");
export const CheckIcon = createIcon(Check, 2, "check");
export const CopyIcon = createIcon(Copy, 1.5, "copy");
export const RocketIcon = createIcon(Rocket);
export const PlayIcon = createControlIcon(Play, 1.75, "play");
export const PauseIcon = createControlIcon(Pause, 1.75, "pause");
export const StopIcon = createControlIcon(Square, 1.75, "stop");
export const ClockIcon = createIcon(Clock3);
export const SettingsIcon = createIcon(Settings2, 1.5, "sliders-horizontal");
export const EditIcon = createIcon(Pencil, 1.5, "pencil");
export const MoreIcon = createIcon(EllipsisVertical, 1.5, "more-vertical");
export const MenuIcon = createIcon(Menu, 1.5, "menu");
export const CloseIcon = createIcon(X, 1.5, "close");
export const SearchIcon = createIcon(Search, 1.5, "search");
export const SplitViewIcon = createIcon(LayoutPanelLeft);
export const WindowViewIcon = createIcon(AppWindow);
export const ExpandIcon = createIcon(Maximize2, 1.5, "maximize");
export const CollapseIcon = createIcon(Minimize2);
export const MaximizeIcon = createIcon(Maximize2, 1.5, "maximize");
export const MonitorIcon = createIcon(Monitor, 1.5, "monitor");
export const TextSizeIcon = createIcon(TextCursorInput, 2);
export const CompassIcon = createIcon(Compass);
export const PlusIcon = createIcon(Plus, 1.5, "plus");
export const HomeIcon = createIcon(House);
export const FolderIcon = createIcon(Folder, 1.5, "folder");
export const DownloadIcon = createIcon(Download);
export const SunIcon = createIcon(Sun);
export const MoonIcon = createIcon(Moon);
export const TerminalIcon = createIcon(SquareTerminal, 1.5, "terminal");
export const BookmarkIcon = createIcon(Bookmark, 1.9, "bookmark");
export const InboxIcon = createIcon(Inbox, 1.5, "inbox");
export const RefreshIcon = createIcon(RefreshCw);
export const TrashIcon = createIcon(Trash2);
export const ChevronDownIcon = createIcon(ChevronDown, 1.5, "chevron-down");
export const SendIcon = createIcon(Send, 1.5, "send");
export const AttachmentIcon = createIcon(Paperclip, 1.5, "attachment");
export const VoiceWaveformIcon = createIcon(AudioLines, 1.8);
export const MicrophoneIcon = createIcon(Mic, 1.5, "mic");
export const MicrophoneOffIcon = createIcon(MicOff, 1.5, "mic-off");
export const VolumeIcon = createIcon(Volume2);
export const HeadphonesIcon = createIcon(Headphones, 1.8);
export const CameraIcon = createIcon(Camera, 1.9);
export const ReplyIcon = createIcon(Reply, 1.5, "undo");
export const PowerIcon = createIcon(Power);
export const AlertIcon = createIcon(CircleAlert, 2);
export const AccountIcon = createIcon(CircleUserRound, 1.8);
export const InfoIcon = createIcon(Info, 2, "circle-info");
export const ImageIcon = createIcon(Image);
export const FileIcon = createIcon(File);
export const FileCheckIcon = createIcon(FileCheck, 1.9);
export const FileTextIcon = createIcon(FileText, 1.8);
export const MessageSquareIcon = createIcon(MessageSquare);
export const MessageSquareQuoteIcon = createIcon(MessageSquareQuote);
export const MessageSquareTextIcon = createIcon(MessageSquareText, 1.5, "message-text");
export const BellIcon = createIcon(Bell);
export const SparklesIcon = createIcon(Sparkles, 1.5, "sparkles");
export const ArrowDownIcon = createIcon(ArrowDown, 2);
export const ChevronLeftIcon = createIcon(ChevronLeft, 1.5, "chevron-left");
export const ChevronRightIcon = createIcon(ChevronRight, 1.5, "chevron-right");
export const GlobeIcon = createIcon(Globe);
export const GripIcon = createIcon(GripVertical);

// These call sites previously used lucide directly, without our stroke defaults.
export function ConnectLinkIcon(props: IconProps) {
  return renderIcon(Link2, "link", props);
}

// Match lucide's defaults here: these controls already used it directly.
export function ComboboxCaretIcon(props: IconProps) {
  return renderIcon(ChevronsUpDown, "chevron-down", props);
}
export function ConnectMonitorIcon(props: IconProps) {
  return renderIcon(Monitor, "monitor", props);
}
export function ConnectLockIcon(props: IconProps) {
  return renderIcon(LockKeyhole, "lock", props);
}
export function GroupArchiveIcon(props: IconProps) {
  return renderIcon(Archive, "archive", props);
}
export function GroupArchiveRestoreIcon(props: IconProps) {
  return renderIcon(ArchiveRestore, "archive-restore", props);
}
export function GroupDeleteIcon(props: IconProps) {
  return renderIcon(Trash2, "trash", props);
}
export function VoiceMoreIcon(props: IconProps) {
  return renderIcon(Ellipsis, "more-horizontal", props);
}
export function WorkMessagesIcon(props: IconProps) {
  return renderIcon(MessageSquare, "message", props);
}
export function WorkGridIcon(props: IconProps) {
  return renderIcon(LayoutGrid, "grid-2x2-2", props);
}
export function InboxEmptyIcon(props: IconProps) {
  return renderIcon(Mail, "inbox", props);
}
export function SecretAddIcon(props: IconProps) {
  return renderIcon(Plus, "plus", props);
}
export function SecretEditIcon(props: IconProps) {
  return renderIcon(Pencil, "pencil", props);
}
export function SecretDeleteIcon(props: IconProps) {
  return renderIcon(Trash2, "trash", props);
}
export function SecretUndoIcon(props: IconProps) {
  return renderIcon(Undo2, "undo", props);
}
export function SecretClearIcon(props: IconProps) {
  return renderIcon(X, "close", props);
}
export function SecretPasteIcon(props: IconProps) {
  return renderIcon(ClipboardPaste, "clipboard", props);
}
