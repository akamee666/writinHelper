// DOM Elements
const editor = document.getElementById('editor');
const suggestionsList = document.getElementById('suggestions-list');
const focusMode = document.getElementById('focus-mode');
const filterBtns = document.querySelectorAll('.filter-btn');

// WebSocket connection
let socket;
let debounceTimer;
let currentText = '';
let appliedSuggestions = new Set();
let textHighlights = [];

// Initialize the app
function init() {
    // Clear default text on first click
    editor.addEventListener('focus', function() {
        if (editor.innerText === 'Write or paste your text here. I\'ll analyze it for improvements without changing your ideas.') {
            editor.innerText = '';
        }
    }, { once: true });

    // Setup event listeners
    editor.addEventListener('input', handleTextInput);
    focusMode.addEventListener('change', handleFocusChange);
    
    filterBtns.forEach(btn => {
        btn.addEventListener('click', handleFilterChange);
    });

    // Initialize WebSocket connection
    connectWebSocket();
}

// Connect to WebSocket server
function connectWebSocket() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws`;
    
    socket = new WebSocket(wsUrl);
    
    socket.onopen = () => {
        console.log('WebSocket connection established');
    };
    
    socket.onmessage = (event) => {
        handleSocketMessage(event.data);
    };
    
    socket.onclose = () => {
        console.log('WebSocket connection closed, attempting to reconnect...');
        setTimeout(connectWebSocket, 2000);
    };
    
    socket.onerror = (error) => {
        console.error('WebSocket error:', error);
    };
}

// Handle text input with debounce
function handleTextInput() {
    clearTimeout(debounceTimer);
    
    debounceTimer = setTimeout(() => {
        currentText = editor.innerText;
        
        if (currentText.trim().length > 10) {
            sendTextForAnalysis();
        } else {
            clearSuggestions();
        }
    }, 1000);
}

// Send text to server for analysis
function sendTextForAnalysis() {
    if (socket && socket.readyState === WebSocket.OPEN) {
        const data = {
            text: currentText,
            focus: focusMode.value
        };
        
        socket.send(JSON.stringify(data));
    }
}

// Handle focus mode change
function handleFocusChange() {
    if (currentText.trim().length > 10) {
        sendTextForAnalysis();
    }
}

// Handle filter button click
function handleFilterChange(e) {
    // Update active button
    filterBtns.forEach(btn => btn.classList.remove('active'));
    e.target.classList.add('active');
    
    const filter = e.target.dataset.filter;
    
    // Filter suggestions list
    const suggestionItems = document.querySelectorAll('.suggestion-item');
    
    suggestionItems.forEach(item => {
        if (filter === 'all' || item.classList.contains(filter)) {
            item.style.display = 'block';
        } else {
            item.style.display = 'none';
        }
    });
}

// Handle WebSocket message
function handleSocketMessage(data) {
    try {
        const response = JSON.parse(data);
        displaySuggestions(response.suggestions);
    } catch (error) {
        console.error('Error parsing server response:', error);
    }
}

// Display suggestions in the panel and highlight text
function displaySuggestions(suggestions) {
    clearSuggestions();
    
    if (!suggestions || suggestions.length === 0) {
        suggestionsList.innerHTML = '<p class="no-suggestions">No suggestions to show. Your writing looks good!</p>';
        return;
    }
    
    // Sort suggestions by position in text (start index)
    suggestions.sort((a, b) => a.start - b.start);
    
    // Create suggestion items and highlights
    suggestions.forEach((suggestion, index) => {
        if (appliedSuggestions.has(suggestionKey(suggestion))) {
            return; // Skip previously applied suggestions
        }
        
        createSuggestionItem(suggestion, index);
        highlightText(suggestion, index);
    });
    
    // Add click listeners to highlights
    setupHighlightListeners();
}

// Create a unique key for a suggestion
function suggestionKey(suggestion) {
    return `${suggestion.start}-${suggestion.end}-${suggestion.suggestion}`;
}

// Create a suggestion item in the suggestions panel
function createSuggestionItem(suggestion, index) {
    const item = document.createElement('div');
    item.className = `suggestion-item ${suggestion.severity}`;
    item.dataset.index = index;
    
    item.innerHTML = `
        <div class="suggestion-category">${capitalizeFirst(suggestion.category)}</div>
        <div class="suggestion-original">"${truncateText(suggestion.original)}"</div>
        <div class="suggestion-replacement">"${truncateText(suggestion.suggestion)}"</div>
        <div class="suggestion-reason">${suggestion.reason}</div>
        <div class="suggestion-actions">
            <button class="action-btn ignore">Ignore</button>
            <button class="action-btn apply">Apply</button>
        </div>
    `;
    
    suggestionsList.appendChild(item);
    
    // Add event listeners to buttons
    item.querySelector('.action-btn.apply').addEventListener('click', () => {
        applySuggestion(suggestion, index);
    });
    
    item.querySelector('.action-btn.ignore').addEventListener('click', () => {
        ignoreSuggestion(suggestion, index);
    });
}

// Highlight text in the editor
function highlightText(suggestion, index) {
    // Create a text range for the current content
    const text = editor.innerText;
    const range = document.createRange();
    const textNodes = getTextNodes(editor);
    
    // Find the text node and offset for the suggestion
    let currentIndex = 0;
    let startNode = null;
    let startOffset = 0;
    let endNode = null;
    let endOffset = 0;
    
    // Find the start and end nodes/offsets
    for (const node of textNodes) {
        const nodeLength = node.textContent.length;
        
        // Find start node and offset
        if (!startNode && currentIndex + nodeLength > suggestion.start) {
            startNode = node;
            startOffset = suggestion.start - currentIndex;
        }
        
        // Find end node and offset
        if (!endNode && currentIndex + nodeLength >= suggestion.end) {
            endNode = node;
            endOffset = suggestion.end - currentIndex;
            break;
        }
        
        currentIndex += nodeLength;
    }
    
    if (startNode && endNode) {
        // Create the highlight span
        const highlight = document.createElement('span');
        highlight.className = `highlight ${suggestion.severity}`;
        highlight.dataset.index = index;
        highlight.title = `Suggestion: ${suggestion.suggestion}`;
        
        // Set the range and insert the highlight
        range.setStart(startNode, startOffset);
        range.setEnd(endNode, endOffset);
        range.surroundContents(highlight);
        
        // Store highlight reference
        textHighlights.push(highlight);
    }
}

// Get all text nodes within an element
function getTextNodes(node) {
    const textNodes = [];
    
    function traverse(node) {
        if (node.nodeType === Node.TEXT_NODE) {
            textNodes.push(node);
        } else {
            for (const child of node.childNodes) {
                traverse(child);
            }
        }
    }
    
    traverse(node);
    return textNodes;
}

// Setup click listeners for text highlights
function setupHighlightListeners() {
    document.querySelectorAll('.highlight').forEach(highlight => {
        highlight.addEventListener('click', () => {
            const index = highlight.dataset.index;
            const suggestionItem = document.querySelector(`.suggestion-item[data-index="${index}"]`);
            
            if (suggestionItem) {
                suggestionItem.scrollIntoView({ behavior: 'smooth', block: 'center' });
                suggestionItem.classList.add('flashing');
                setTimeout(() => suggestionItem.classList.remove('flashing'), 1500);
            }
        });
    });
}

// Apply a suggestion
function applySuggestion(suggestion, index) {
    // Find the highlight element
    const highlight = document.querySelector(`.highlight[data-index="${index}"]`);
    
    if (highlight) {
        // Replace the text
        highlight.innerText = suggestion.suggestion;
        highlight.classList.remove('highlight', 'critical', 'suggestion', 'optional');
        
        // Mark as applied
        appliedSuggestions.add(suggestionKey(suggestion));
        
        // Remove the suggestion item
        const suggestionItem = document.querySelector(`.suggestion-item[data-index="${index}"]`);
        if (suggestionItem) {
            suggestionItem.remove();
        }
        
        // Update current text
        currentText = editor.innerText;
        
        // Analyze text again after a short delay
        setTimeout(() => {
            sendTextForAnalysis();
        }, 1500);
    }
}

// Ignore a suggestion
function ignoreSuggestion(suggestion, index) {
    // Find and remove the highlight
    const highlight = document.querySelector(`.highlight[data-index="${index}"]`);
    
    if (highlight) {
        // Replace the highlight with its text content
        const text = highlight.innerText;
        const textNode = document.createTextNode(text);
        highlight.parentNode.replaceChild(textNode, highlight);
    }
    
    // Mark as applied (ignored)
    appliedSuggestions.add(suggestionKey(suggestion));
    
    // Remove the suggestion item
    const suggestionItem = document.querySelector(`.suggestion-item[data-index="${index}"]`);
    if (suggestionItem) {
        suggestionItem.remove();
    }
}

// Clear all suggestions and highlights
function clearSuggestions() {
    // Clear suggestions panel
    suggestionsList.innerHTML = '';
    
    // Remove highlights from text
    textHighlights.forEach(highlight => {
        if (highlight.parentNode) {
            const text = highlight.innerText;
            const textNode = document.createTextNode(text);
            highlight.parentNode.replaceChild(textNode, highlight);
        }
    });
    
    textHighlights = [];
}

// Helper function to capitalize first letter
function capitalizeFirst(str) {
    return str.charAt(0).toUpperCase() + str.slice(1);
}

// Helper function to truncate text
function truncateText(text, maxLength = 50) {
    if (text.length <= maxLength) return text;
    return text.substring(0, maxLength) + '...';
}

// Initialize the app when the DOM is loaded
document.addEventListener('DOMContentLoaded', init);
