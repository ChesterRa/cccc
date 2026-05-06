import { useMemo, useEffect, useRef } from 'react';
import MarkdownIt from 'markdown-it';
import { renderToStaticMarkup } from 'react-dom/server';
import { CheckIcon, CopyIcon } from './Icons';
import { classNames } from '../utils/classNames';
import { copyTextToClipboard } from '../utils/copy';

const copyIconMarkup = renderToStaticMarkup(<CopyIcon className="w-3.5 h-3.5" strokeWidth={2} aria-hidden="true" />);
const copiedIconMarkup = renderToStaticMarkup(<CheckIcon className="w-3.5 h-3.5" strokeWidth={2} aria-hidden="true" />);

interface MarkdownRendererProps {
    content: string;
    isDark?: boolean;
    className?: string;
    /** Force light text (for colored backgrounds like user messages) */
    invertText?: boolean;
}

export function MarkdownRenderer({ content, isDark, className, invertText }: MarkdownRendererProps) {
    const containerRef = useRef<HTMLDivElement>(null);

    const md = useMemo(() => {
        const instance = new MarkdownIt({
            html: false, // Security: Disable raw HTML to prevent XSS
            linkify: true,
            typographer: true,
            breaks: true,
            highlight: (str: string, lang: string): string => {
                const finalLang = lang?.toLowerCase().trim() || 'code';
                const escaped = instance.utils.escapeHtml(str);

                // Render code blocks with a copy button, without syntax highlighting.
                // Toggle state through CSS classes to avoid direct innerHTML edits during React reconciliation.
                return (
                    '<div class="code-block-wrapper relative group">' +
                    '<div class="code-block-header flex items-center justify-between">' +
                    '<span class="uppercase">' + finalLang + '</span>' +
                    '<button class="copy-button flex items-center gap-1 select-none" data-code="' + encodeURIComponent(str) + '">' +
                    '<span class="copy-icon pointer-events-none">' + copyIconMarkup + '</span>' +
                    '<span class="copy-text pointer-events-none">Copy</span>' +
                    '<span class="copied-icon pointer-events-none hidden text-green-500 dark:text-emerald-400">' + copiedIconMarkup + '</span>' +
                    '<span class="copied-text pointer-events-none hidden text-green-500 dark:text-emerald-400">Copied!</span>' +
                    '</button>' +
                    '</div>' +
                    '<pre><code class="language-' + finalLang + '">' + escaped + '</code></pre>' +
                    '</div>'
                );
            },
        });
        return instance;
    }, []);

    const htmlContent = useMemo(() => {
        return md.render(content || "");
    }, [md, content]);

    // Use event delegation for copy handling.
    useEffect(() => {
        const container = containerRef.current;
        if (!container) return;

        const handleCopy = async (e: MouseEvent) => {
            const button = (e.target as HTMLElement).closest('.copy-button');
            if (!button) return;

            e.preventDefault();
            e.stopPropagation();

            const code = decodeURIComponent(button.getAttribute('data-code') || '');
            if (!code) {
                console.error('No code found in data-code attribute');
                return;
            }
            try {
                const copied = await copyTextToClipboard(code);
                if (!copied) throw new Error('copy failed');
                // Toggle state through CSS classes to avoid React DOM sync issues from innerHTML edits.
                button.classList.add('copied', 'pointer-events-none');
                const copyIcon = button.querySelector('.copy-icon');
                const copyText = button.querySelector('.copy-text');
                const copiedIcon = button.querySelector('.copied-icon');
                const copiedText = button.querySelector('.copied-text');
                if (copyIcon) copyIcon.classList.add('hidden');
                if (copyText) copyText.classList.add('hidden');
                if (copiedIcon) copiedIcon.classList.remove('hidden');
                if (copiedText) copiedText.classList.remove('hidden');

                setTimeout(() => {
                    button.classList.remove('copied', 'pointer-events-none');
                    if (copyIcon) copyIcon.classList.remove('hidden');
                    if (copyText) copyText.classList.remove('hidden');
                    if (copiedIcon) copiedIcon.classList.add('hidden');
                    if (copiedText) copiedText.classList.add('hidden');
                }, 2000);
            } catch (err) {
                console.error('Failed to copy code:', err);
            }
        };

        container.addEventListener('click', handleCopy);
        return () => container.removeEventListener('click', handleCopy);
    }, [htmlContent]);

    return (
        <div
            ref={containerRef}
            className={classNames(
                'markdown-body prose max-w-none prose-sm',
                (isDark || invertText) ? 'prose-invert' : '',
                '[&_p]:m-0 [&_ul]:my-1 [&_ol]:my-1',
                '[&_a]:![color:inherit] [&_a]:underline',
                className
            )}
            style={{ color: 'inherit' }}
            dangerouslySetInnerHTML={{ __html: htmlContent }}
        />
    );
}
