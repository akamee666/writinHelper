// DOM Elements
const editor = document.getElementById('editor');
// Update to the new container ID for suggestions
const suggestionsList = document.getElementById('notification-suggestions'); 

// WebSocket connection
let socket;
let debounceTimer;
let currentText = '';
let appliedSuggestions = new Set();
let textHighlights = [];

// Initialize the app
function init() {
    // Setup event listeners
    editor.addEventListener('input', handleTextInput);
    // Removed focusMode and filterBtns listeners

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
        // Send only the text, focus is removed
        const data = {
            text: currentText 
        };
        
        socket.send(JSON.stringify(data));
    }
}

// Removed handleFocusChange function

// Removed handleFilterChange function

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
    item.dataset.key = suggestionKey(suggestion); // Add key for easy removal

    // Build HTML structure without icons and truncation
    item.innerHTML = `
        <div class="suggestion-actions">
            <button class="action-btn apply-btn" title="Apply Suggestion (Dismiss)">✔️</button>
            <button class="action-btn ignore-btn" title="Ignore Suggestion">❌</button>
        </div>
        <div class="suggestion-header">
            <div class="suggestion-original">"${suggestion.original}"</div>
        </div>
        <div class="suggestion-content">
            <div class="suggestion-hint">"${suggestion.suggestion}"</div>
            <div class="suggestion-reason">${suggestion.reason}</div>
        </div>
    `;
    
    suggestionsList.appendChild(item);

    // Add event listeners for Apply/Ignore buttons
    item.querySelector('.apply-btn').addEventListener('click', () => handleApplySuggestion(item, suggestion));
    item.querySelector('.ignore-btn').addEventListener('click', () => handleIgnoreSuggestion(item, suggestion));
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
            // Query within the new suggestions container
            const suggestionItem = suggestionsList.querySelector(`.suggestion-item[data-index="${index}"]`); 
            
            if (suggestionItem) {
                // Scroll the notification container to show the item
                suggestionItem.scrollIntoView({ behavior: 'smooth', block: 'nearest' }); 
                suggestionItem.classList.add('flashing');
                setTimeout(() => suggestionItem.classList.remove('flashing'), 1500);
            }
        });
    });
}

// Handle Apply Suggestion button click
function handleApplySuggestion(itemElement, suggestion) {
    console.log("Apply clicked (currently dismisses):", suggestion);
    // TODO: Implement actual apply logic IF the user clarifies how it should work with hints.
    // For now, treat Apply the same as Ignore: dismiss the suggestion visually.
    appliedSuggestions.add(suggestionKey(suggestion)); // Mark as "applied" (ignored)
    itemElement.remove(); // Remove the suggestion item from the list
    // Optionally, remove the corresponding highlight in the editor
    const highlight = textHighlights.find(h => h.dataset.index === itemElement.dataset.index);
    if (highlight && highlight.parentNode) {
         // Just remove the highlight styling/wrapper, keep the text
         const text = highlight.innerText;
         const textNode = document.createTextNode(text);
         highlight.parentNode.replaceChild(textNode, highlight);
         // Remove from our tracking array
         textHighlights = textHighlights.filter(h => h !== highlight);
    }
}

// Handle Ignore Suggestion button click
function handleIgnoreSuggestion(itemElement, suggestion) {
    console.log("Ignore clicked:", suggestion);
    appliedSuggestions.add(suggestionKey(suggestion)); // Mark as ignored
    itemElement.remove(); // Remove the suggestion item from the list
     // Optionally, remove the corresponding highlight in the editor
    const highlight = textHighlights.find(h => h.dataset.index === itemElement.dataset.index);
     if (highlight && highlight.parentNode) {
         // Just remove the highlight styling/wrapper, keep the text
         const text = highlight.innerText;
         const textNode = document.createTextNode(text);
         highlight.parentNode.replaceChild(textNode, highlight);
         // Remove from our tracking array
         textHighlights = textHighlights.filter(h => h !== highlight);
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
    // Reset ignored suggestions set when clearing all
    appliedSuggestions.clear(); 
    textHighlights = [];
}

// Helper function to capitalize first letter
function capitalizeFirst(str) {
    return str.charAt(0).toUpperCase() + str.slice(1);
}

// Initialize the app when the DOM is loaded
document.addEventListener('DOMContentLoaded', init);
