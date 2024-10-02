// Theme Toggle Logic

// Function to initialize the theme
function initializeTheme() {
    const themeToggle = document.getElementById('themeToggle');
    const themeIcon = document.getElementById('themeIcon');
    const body = document.body;
    const savedTheme = localStorage.getItem('theme') || 'light-mode';

    body.classList.add(savedTheme);
    setThemeIcon(body, themeIcon);

    // Theme Toggle Event
    themeToggle.addEventListener('click', () => {
        const currentTheme = body.classList.contains('light-mode') ? 'light-mode' : 'dark-mode';
        const newTheme = currentTheme === 'light-mode' ? 'dark-mode' : 'light-mode';

        body.classList.remove(currentTheme);
        body.classList.add(newTheme);
        localStorage.setItem('theme', newTheme);

        setThemeIcon(body, themeIcon);
    });
}

// Function to set the theme icon based on the current theme
function setThemeIcon(body, themeIcon) {
    if (body.classList.contains('dark-mode')) {
        // Dark mode icon (moon)
        themeIcon.innerHTML = `
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                d="M17.293 13.293A8 8 0 116.707 2.707
                8 8 0 0017.293 13.293z" />
        `;
    } else {
        // Light mode icon (sun)
        themeIcon.innerHTML = `
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                d="M12 3v1m0 16v1M4.22 4.22l.71.71
                M19.07 19.07l.71.71M1 12h1M21 12h1
                M4.22 19.78l.71-.71M19.07 4.93l.71-.71
                M12 5a7 7 0 100 14 7 7 0 000-14z" />
        `;
    }
}

// Initialize theme when the DOM content is loaded
document.addEventListener('DOMContentLoaded', initializeTheme);
