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
    });
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


// Helper function to capitalize first letter
function capitalizeFirst(str) {
    return str.charAt(0).toUpperCase() + str.slice(1);
}

// Initialize the app when the DOM is loaded
document.addEventListener('DOMContentLoaded', init);
