import type { LucideIcon, LucideProps } from "lucide-react";
import {
  AppWindow,
  ArrowDown,
  Bookmark,
  Bell,
  Camera,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Clock3,
  Check,
  Clipboard,
  Compass,
  Copy,
  EllipsisVertical,
  File,
  Folder,
  Globe,
  GripVertical,
  House,
  Image,
  Inbox,
  Info,
  LayoutPanelLeft,
  Maximize2,
  Menu,
  Mic,
  MessageSquare,
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
  Download,
  X,
} from "lucide-react";

type IconProps = Omit<LucideProps, "ref">;

function createIcon(Icon: LucideIcon, defaultStrokeWidth = 1.5) {
  function WrappedIcon({
    size = 18,
    strokeWidth = defaultStrokeWidth,
    absoluteStrokeWidth = true,
    ...props
  }: IconProps) {
    return (
      <Icon
        size={size}
        strokeWidth={strokeWidth}
        absoluteStrokeWidth={absoluteStrokeWidth}
        {...props}
      />
    );
  }

  WrappedIcon.displayName = `Wrapped${Icon.displayName || "Icon"}`;
  return WrappedIcon;
}

function createControlIcon(Icon: LucideIcon, defaultStrokeWidth = 1.75) {
  function WrappedIcon({
    size = 18,
    strokeWidth = defaultStrokeWidth,
    absoluteStrokeWidth = true,
    ...props
  }: IconProps) {
    return (
      <Icon
        size={size}
        strokeWidth={strokeWidth}
        absoluteStrokeWidth={absoluteStrokeWidth}
        fill="none"
        {...props}
      />
    );
  }

  WrappedIcon.displayName = `Control${Icon.displayName || "Icon"}`;
  return WrappedIcon;
}

export const ClipboardIcon = createIcon(Clipboard);
export const CheckIcon = createIcon(Check, 2);
export const CopyIcon = createIcon(Copy);
export const RocketIcon = createIcon(Rocket);
export const PlayIcon = createControlIcon(Play);
export const PauseIcon = createControlIcon(Pause);
export const StopIcon = createControlIcon(Square);
export const ClockIcon = createIcon(Clock3);
export const SettingsIcon = createIcon(Settings2);
export const EditIcon = createIcon(Pencil);
export const MoreIcon = createIcon(EllipsisVertical);
export const MenuIcon = createIcon(Menu);
export const CloseIcon = createIcon(X);
export const SearchIcon = createIcon(Search);
export const SplitViewIcon = createIcon(LayoutPanelLeft);
export const WindowViewIcon = createIcon(AppWindow);
export const ExpandIcon = createIcon(Maximize2);
export const CollapseIcon = createIcon(Minimize2);
export const MaximizeIcon = createIcon(Maximize2);
export const MonitorIcon = createIcon(Monitor);
export const TextSizeIcon = createIcon(TextCursorInput, 2);
export const CompassIcon = createIcon(Compass);
export const PlusIcon = createIcon(Plus);
export const HomeIcon = createIcon(House);
export const FolderIcon = createIcon(Folder);
export const DownloadIcon = createIcon(Download);
export const SunIcon = createIcon(Sun);
export const MoonIcon = createIcon(Moon);
export const TerminalIcon = createIcon(SquareTerminal);
export const BookmarkIcon = createIcon(Bookmark, 1.9);
export const InboxIcon = createIcon(Inbox);
export const RefreshIcon = createIcon(RefreshCw);
export const TrashIcon = createIcon(Trash2);
export const ChevronDownIcon = createIcon(ChevronDown);
export const SendIcon = createIcon(Send);
export const AttachmentIcon = createIcon(Paperclip);
export const MicrophoneIcon = createIcon(Mic);
export const CameraIcon = createIcon(Camera, 1.9);
export const ReplyIcon = createIcon(Reply);
export const PowerIcon = createIcon(Power);
export const AlertIcon = createIcon(CircleAlert, 2);
export const InfoIcon = createIcon(Info, 2);
export const ImageIcon = createIcon(Image);
export const FileIcon = createIcon(File);
export const MessageSquareIcon = createIcon(MessageSquare);
export const MessageSquareTextIcon = createIcon(MessageSquareText);
export const BellIcon = createIcon(Bell);
export const SparklesIcon = createIcon(Sparkles);
export const ArrowDownIcon = createIcon(ArrowDown, 2);
export const ChevronLeftIcon = createIcon(ChevronLeft);
export const ChevronRightIcon = createIcon(ChevronRight);
export const GlobeIcon = createIcon(Globe);
export const GripIcon = createIcon(GripVertical);
