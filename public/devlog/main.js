(function() {
    'use strict';
    function getPreferredTheme() {
        var stored = localStorage.getItem('xavier-theme');
        if (stored) return stored;
        return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    }
    function setTheme(theme) {
        document.documentElement.setAttribute('data-theme', theme);
        localStorage.setItem('xavier-theme', theme);
    }
    setTheme(getPreferredTheme());
    var toggleBtn = document.getElementById('theme-toggle');
    if (toggleBtn) {
        toggleBtn.addEventListener('click', function() {
            var current = document.documentElement.getAttribute('data-theme');
            setTheme(current === 'dark' ? 'light' : 'dark');
        });
    }
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function(e) {
        if (!localStorage.getItem('xavier-theme')) {
            setTheme(e.matches ? 'dark' : 'light');
        }
    });
    var searchInput = document.getElementById('post-search');
    var postList = document.getElementById('post-list');
    if (searchInput && postList) {
        var items = postList.querySelectorAll('li');
        var noResults = document.getElementById('no-results');
        searchInput.addEventListener('input', function() {
            var query = this.value.toLowerCase().trim();
            var visibleCount = 0;
            items.forEach(function(item) {
                var title = item.querySelector('.post-title');
                var desc = item.querySelector('.post-desc');
                var tags = item.querySelector('.post-tags-inline');
                var text = (title ? title.textContent : '') + ' ' +
                           (desc ? desc.textContent : '') + ' ' +
                           (tags ? tags.textContent : '');
                var matches = !query || text.toLowerCase().includes(query);
                item.style.display = matches ? '' : 'none';
                if (matches) visibleCount++;
            });
            if (noResults) {
                noResults.style.display = visibleCount === 0 ? 'block' : 'none';
            }
        });
    }
    console.log('Xavier DevLog loaded.');
})();
