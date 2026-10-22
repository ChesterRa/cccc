() => {
    const messages = [];
    const selectors = '[data-message-author-role="user"], [data-message-author-role="assistant"], '
        + 'main [data-user-message-bubble], main [data-conversation-role="assistant"]';
    for (const element of document.querySelectorAll(selectors)) {
        const legacy = element.closest('[data-message-author-role]');
        if (legacy) {
            if (legacy === element) messages.push({role: element.dataset.messageAuthorRole, node: element});
            continue;
        }
        if (element.hasAttribute('data-user-message-bubble')) {
            // A bubble inside an assistant's unit is not an independent user turn.
            const unit = element.closest('[data-content-search-unit-key]');
            if (!unit?.querySelector('[data-conversation-role="assistant"]')) {
                messages.push({role: 'user', node: element});
            }
        } else {
            // Current ChatGPT renders an accessible role heading immediately
            // before the assistant body. Never read the whole turn: it also
            // contains the user's prompt, which cannot prove a returned receipt.
            const node = element.nextElementSibling;
            if (!element.closest('[data-user-message-bubble]') && node
                && !node.matches('[data-user-message-bubble], [data-message-author-role]')
                && !node.querySelector('[data-user-message-bubble], [data-conversation-role], [data-message-author-role]')) {
                messages.push({role: 'assistant', node});
            }
        }
    }
    return messages.sort((a, b) => a.node.compareDocumentPosition(b.node) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1);
}
