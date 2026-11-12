/* Own Keyboard Switch — project page behaviour: language, typing demo, scroll motion. */
(function () {
  'use strict';

  var doc = document;
  var root = doc.documentElement;
  var reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  var $ = function (sel, el) { return (el || doc).querySelector(sel); };
  var $$ = function (sel, el) { return Array.prototype.slice.call((el || doc).querySelectorAll(sel)); };

  /* ------------------------------------------------------------------ *
   * Language. Russian lives in the HTML; English is the table below.
   * ------------------------------------------------------------------ */
  var EN = {
    'page.title': 'Own Keyboard Switch — automatic RU/EN keyboard layout switching',
    'page.desc': 'Own Keyboard Switch fixes words typed in the wrong keyboard layout: ghbdtn → привет, руддщ → hello. Windows and Linux, no ads, typed text never leaves the computer.',
    'nav.label': 'Sections',
    'nav.how': 'How it works',
    'nav.features': 'Features',
    'nav.tour': 'Settings',
    'nav.keys': 'Keys',
    'nav.faq': 'FAQ',
    'nav.download': 'Download',

    'hero.eyebrow': 'Windows and Linux · 0.6.1-beta',
    'hero.title': 'Type as&nbsp;usual. <span class="grad">The program fixes the layout.</span>',
    'hero.lead': 'Own Keyboard Switch fixes words typed in the wrong Russian or English layout. When a word is finished, the program decides which language it is written in, erases it, switches the layout and types the word again.',
    'hero.download': 'Download 0.6.1-beta',
    'hero.guide': 'User guide',
    'hero.fact1': 'Text never leaves the computer',
    'hero.fact2': 'Dictionaries are built in',
    'hero.fact3': 'No ads',

    'demo.title': 'What you type',
    'demo.note': 'The same keys, two readings. The one that is a word stays.',
    'demo.sr': 'Example: you type ghbdtn in the English layout; the program replaces it with «привет» and turns on the Russian layout.',
    'demo.word': 'a word',
    'demo.noword': 'not a word',
    'shot.alt': 'The Own Keyboard Switch settings window, General section',

    'how.eyebrow': 'How auto switch works',
    'how.title': 'The program remembers the keys, not&nbsp;the&nbsp;text on&nbsp;screen',
    'how.s1.t': 'You finish a word',
    'how.s1.p': 'Space, Enter, Tab or a character that is not a letter in either the Russian or the English layout.',
    'how.s2.t': 'Two readings are compared',
    'how.s2.p': 'Whether each one is in the dictionary and how frequent the word is, how usual the letter combinations are for each language, what the built-in rules say.',
    'how.s3.t': 'The word is typed again',
    'how.s3.p': 'If one reading is a real word and the other is nonsense, the word is erased and typed in the other layout. The layout is switched.',
    'how.term.t': 'In a terminal, Enter waits',
    'how.term.p': 'Enter runs the command at once, so the program holds it back while it checks the last word and passes it to the terminal after the correction. It knows Windows and Linux commands and words of programming languages even if they are not in the ordinary dictionary.',
    'how.skip.t': 'When the program does not intervene',
    'how.skip.p': 'A word is deliberately left as it is if:',
    'how.skip.1': 'Backspace was pressed before the word — you are editing the text yourself;',
    'how.skip.2': 'you changed the layout yourself before the word;',
    'how.skip.3': 'both readings look like words — the program does not argue with a word that exists;',
    'how.skip.4': 'the window is an excluded program or the field is a password field;',
    'how.skip.5': 'a layout other than Russian or English is turned on.',

    'feat.eyebrow': 'What the program does',
    'feat.title': 'More than the layout',
    'feat.1.t': 'Auto switch',
    'feat.1.p': 'The decision is based on built-in dictionaries, word frequencies, letter combination statistics and additional rules. One-letter words are fixed too: z → я.',
    'feat.2.t': 'Undo with one key',
    'feat.2.p': 'Break undoes the last correction or converts a word the program left alone. After several undos in a row the program suggests a rule for that word.',
    'feat.3.t': 'Passwords',
    'feat.3.p': 'A password typed in the Russian layout is retyped in English: ИшпМщмф2024! → BigVova2024!. In a password field the English layout is turned on from the first character.',
    'feat.4.t': 'Letter case',
    'feat.4.p': 'Fixes two capitals at the start of a word, text typed with Caps Lock turned on by accident, and abbreviations in the wrong layout: ШЫЩ → ISO.',
    'feat.5.t': 'Working with text',
    'feat.5.p': 'Changing the layout and case of, and transliterating, the selected text and the clipboard; pasting without formatting; numbers in words.',
    'feat.6.t': 'Autoreplace',
    'feat.6.p': 'Abbreviations expand into ready-made text, including multi-line text, with a remembered caret position. An abbreviation is recognised in the other layout too.',
    'feat.7.t': 'Spell checking',
    'feat.7.p': 'On command or right while typing: suggestions at the caret or automatic fixes of unambiguous mistakes. Checking runs on the computer, with Hunspell dictionaries.',
    'feat.8.t': 'Clipboard and indicator',
    'feat.8.p': 'A history of the last 30 copied texts, a floating layout indicator, flags in the tray, sounds and switching the layout with a single key.',

    'tour.eyebrow': 'The settings window',
    'tour.title': 'Everything is adjustable',
    'tour.1.t': 'General',
    'tour.1.p': 'Startup, auto switch, password recognition, a flag icon, the floating indicator. The interface is in English and Russian, with light, dark and system themes.',
    'tour.2.t': 'Switching rules',
    'tour.2.p': 'Your own rules for words: “Convert to the other layout” or “Do not convert”. Rules can be exported to a file and imported back. “Improve switching” recognises more short English words after a Russian word.',
    'tour.3.t': 'Excluded programs',
    'tour.3.p': 'In the chosen programs words are not checked at all. Terminals and IDEs are offered on the first start.',
    'tour.4.t': 'Autoreplace',
    'tour.4.p': 'Replace by a tooltip, Space, Enter, Tab or a hotkey. Entries can also be inserted without abbreviations — from a floating list or a menu at the caret.',
    'tour.5.t': 'Spell checking',
    'tour.5.p': 'Russian and English, your own words, additional dictionaries. An additional dictionary is downloaded with the Download button.',
    'tour.6.t': 'Hotkeys and mouse',
    'tour.6.p': 'Any action can be reassigned, and mouse buttons can be given key combinations. No Break key on the laptop — assign another one.',

    'keys.eyebrow': 'Everything on one page',
    'keys.title': 'Six combinations worth remembering',
    'keys.1': 'Undo a correction or convert a word by hand — right after the word',
    'keys.2': 'Convert the selected text to the other layout',
    'keys.3': 'Change the case of the selected text',
    'keys.4': 'Transliterate the selected text: Привет ↔ Privet',
    'keys.5': 'Paste without formatting',
    'keys.dbl': '2 × click',
    'keys.6': 'On the tray icon — open the settings (in KDE Plasma, a single click)',
    'keys.note': 'The combinations are changed in the Hotkeys section.',

    'priv.eyebrow': 'What the program does not do',
    'priv.title': 'Typed text stays on&nbsp;the&nbsp;computer',
    'priv.1.t': 'It does not send text over the network',
    'priv.1.p': 'Dictionaries and language models are built in. The network is needed to download an additional spelling dictionary when you click Download yourself, and once a week to ask GitHub whether a newer version exists. Nothing about you is sent; the check can be turned off. No ads.',
    'priv.2.t': 'It does not write text to the log',
    'priv.2.p': 'The log holds only technical details: how many letters the word had, what decision was made and why. The log is off until you turn it on yourself.',
    'priv.3.t': 'Russian and English only',
    'priv.3.p': 'The program works with the Russian–English pair. Other layouts can stay in the system: by default the program does not touch input in them.',

    'dl.eyebrow': 'Download · 0.6.1-beta',
    'dl.title': 'Windows 10/11 and Linux',
    'dl.lead': 'A prerelease. The Linux version is still young: reports are welcome.',
    'dl.1': 'Put the file in a writable folder and run it. No installation required.',
    'dl.2.t': 'Installer',
    'dl.2': 'A current-user or all-users installation, shortcuts and startup options.',
    'dl.3.t': '.deb package',
    'dl.3': 'Ubuntu 24.04 or newer. Adds the program to the application menu.',
    'dl.4': 'Make it executable and run it; settings are kept next to the file.',
    'dl.linux.t': 'Linux: where it works',
    'dl.linux.1': 'Any desktop, nothing to install.',
    'dl.linux.2': 'A bundled GNOME Shell extension; sign out and back in once after the first start.',
    'dl.linux.3': 'The KDE layout service and a bundled KWin script.',
    'dl.linux.note': 'Sway, Hyprland and other Wayland compositors are not supported yet.',
    'dl.cmd.t': 'Installing on Linux',
    'dl.cmd.note': 'On the first start the program asks for permission (pkexec) to give your session access to the keyboards. The program itself always runs as a regular user.',
    'dl.all': 'All releases and SHA-256 checksums',
    'dl.changes': 'Changelog',
    'dl.install': 'Installation in detail',
    'copy': 'Copy',
    'copied': 'Copied',

    'faq.eyebrow': 'Solving problems and FAQ',
    'faq.title': 'If something goes wrong',
    'faq.1.q': 'A word was not corrected',
    'faq.1.a': 'Most often Backspace was pressed before the word, the layout was switched by hand right before the word, the program is in the exclusions, or both readings look like words (рук — her). Press Break to convert the word now, and add a “Convert to the other layout” rule if the word should always be corrected.',
    'faq.2.q': 'The program corrects what it should not',
    'faq.2.a': 'Press Break — the correction is undone. After two undos in a row the program offers a “Do not convert” rule. For terms of one program, add it to the exclusions; for short words, increase “Do not convert words shorter than, letters” or move the analysis sensitivity towards “cautious”.',
    'faq.3.q': 'It does not work in some Windows windows',
    'faq.3.a': 'Windows does not let an ordinary program read the keyboard in windows running as administrator: Task Manager, installers, an administrator console. Turn on “General → Run with administrator rights”.',
    'faq.4.q': 'The Break key does not work or is missing',
    'faq.4.a': 'Many laptops have no Break (Pause) key, or it is pressed via Fn. Assign another combination to the “Undo layout conversion” action in the Hotkeys section.',
    'faq.5.q': 'The antivirus complains',
    'faq.5.a': 'A layout switcher by its nature watches the keyboard and sends key presses, so antivirus heuristics sometimes react to it. Download the program only from the Releases page and check the SHA-256 checksums published next to the files.',
    'faq.6.q': 'Linux: keys are not captured',
    'faq.6.a': 'Open the setup window — it appears at startup if something is not ready — and go through the flagged items: keyboard access, desktop integration, layouts, active session. The okbswitch --diagnose command reports the session, the layouts and the device access.',
    'faq.more': 'All questions in the guide',
    'faq.issue': 'Report a bug',

    'foot.license': 'The project’s own code is under the PolyForm Noncommercial 1.0.0 license: free for noncommercial use.',
    'foot.guide': 'User guide',
    'foot.releases': 'Releases',
    'foot.lic': 'License'
  };

  var RU = {
    'page.title': doc.title,
    'page.desc': $('meta[name="description"]').getAttribute('content'),
    'demo.word': 'слово',
    'demo.noword': 'не слово',
    'copied': 'Скопировано'
  };
  var DICT = { ru: RU, en: EN };
  var SITE = $('link[rel="alternate"][hreflang="ru"]').getAttribute('href');
  var lang = 'ru';

  $$('[data-i18n]').forEach(function (el) { RU[el.dataset.i18n] = el.innerHTML; });
  $$('[data-i18n-alt]').forEach(function (el) { RU[el.dataset.i18nAlt] = el.getAttribute('alt'); });
  $$('[data-i18n-aria]').forEach(function (el) { RU[el.dataset.i18nAria] = el.getAttribute('aria-label'); });
  $$('[data-href-en]').forEach(function (el) { el.dataset.hrefRu = el.getAttribute('href'); });

  /* The version the page was built for; replaced by the newest release after load. */
  var BUILT = root.dataset.version;
  var version = BUILT;
  function withVersion(text) { return version === BUILT ? text : text.split(BUILT).join(version); }
  function t(key) { return withVersion(DICT[lang][key] || RU[key] || ''); }

  function applyLang(next, remember) {
    lang = next === 'en' ? 'en' : 'ru';
    root.lang = lang;
    doc.title = t('page.title');
    $('meta[name="description"]').setAttribute('content', t('page.desc'));
    $$('[data-i18n]').forEach(function (el) { el.innerHTML = t(el.dataset.i18n); });
    $$('[data-i18n-alt]').forEach(function (el) { el.setAttribute('alt', t(el.dataset.i18nAlt)); });
    $$('[data-i18n-aria]').forEach(function (el) { el.setAttribute('aria-label', t(el.dataset.i18nAria)); });
    $$('[data-href-en]').forEach(function (el) {
      el.setAttribute('href', lang === 'en' ? el.dataset.hrefEn : el.dataset.hrefRu);
    });
    $$('img[data-shot]').forEach(function (img) {
      var src = 'img/' + lang + '/' + img.dataset.shot;
      if (img.getAttribute('src') !== src) img.setAttribute('src', src);
    });
    $$('.lang button').forEach(function (b) {
      b.setAttribute('aria-pressed', String(b.dataset.lang === lang));
    });
    refreshVerdicts();
    // The Russian page lives at the site root, the English one at ?lang=en (see hreflang in index.html).
    var url = SITE + (lang === 'en' ? '?lang=en' : '');
    $('link[rel="canonical"]').setAttribute('href', url);
    $('meta[property="og:url"]').setAttribute('content', url);
    $('meta[property="og:locale"]').setAttribute('content', lang === 'en' ? 'en_US' : 'ru_RU');
    if (remember) {
      try { localStorage.setItem('okbs-lang', lang); } catch (e) { /* private mode */ }
      if (window.history && history.replaceState) {
        history.replaceState(null, '', (lang === 'en' ? '?lang=en' : location.pathname) + location.hash);
      }
    }
  }

  function initialLang() {
    var q = /[?&]lang=(ru|en)\b/.exec(location.search);
    if (q) return q[1];
    // Search engines get the language of the address they asked for, so that
    // the root stays Russian for them, as declared by hreflang.
    if (/bot|crawl|spider|slurp|yandex|bing|duckduck|baidu|lighthouse/i.test(navigator.userAgent)) return 'ru';
    try {
      var saved = localStorage.getItem('okbs-lang');
      if (saved === 'ru' || saved === 'en') return saved;
    } catch (e) { /* private mode */ }
    var prefs = navigator.languages || [navigator.language || 'ru'];
    for (var i = 0; i < prefs.length; i++) {
      var p = String(prefs[i]).toLowerCase().slice(0, 2);
      if (p === 'ru' || p === 'uk' || p === 'be' || p === 'kk') return 'ru';
      if (p === 'en') return 'en';
    }
    return 'en';
  }

  $$('.lang button').forEach(function (b) {
    b.addEventListener('click', function () { applyLang(b.dataset.lang, true); });
  });

  /* ------------------------------------------------------------------ *
   * Typing demo: the same keys read in two layouts.
   * ------------------------------------------------------------------ */
  var LAT = "qwertyuiop[]asdfghjkl;'zxcvbnm,.";
  var CYR = 'йцукенгшщзхъфывапролджэячсмитьбю';
  var EXAMPLES = [
    { keys: 'ghbdtn', typed: 'en' },   // ghbdtn -> привет
    { keys: 'hello', typed: 'ru' },    // руддщ -> hello
    { keys: 'mkdir', typed: 'ru' },    // ьлвшк -> mkdir
    { keys: 'cgfcb,j', typed: 'en' },  // cgfcb,j -> спасибо
    { keys: 'git', typed: 'ru' }       // пше -> git
  ];

  var field = $('#demoField');
  var textEl = $('#demoText');
  var keysEl = $('#demoKeys');
  var chip = $('#demoLayout');
  var readEn = $('#readEn');
  var readRu = $('#readRu');
  var demoVisible = false;

  function cyr(ch) { var i = LAT.indexOf(ch); return i < 0 ? ch : CYR[i]; }
  function reading(keys, layout) {
    return layout === 'en' ? keys : keys.split('').map(cyr).join('');
  }
  function sleep(ms) { return new Promise(function (r) { setTimeout(r, ms); }); }
  function whenVisible() {
    return new Promise(function (resolve) {
      (function poll() {
        if (demoVisible && !doc.hidden) resolve(); else setTimeout(poll, 300);
      })();
    });
  }

  function setLayout(layout, flip) {
    chip.dataset.layout = layout;
    chip.textContent = layout.toUpperCase();
    if (flip) {
      chip.classList.remove('flip');
      void chip.offsetWidth;
      chip.classList.add('flip');
    }
  }

  function buildCaps(keys) {
    keysEl.innerHTML = '';
    keysEl.removeAttribute('data-pick');
    keys.split('').forEach(function (ch, i) {
      var cap = doc.createElement('span');
      cap.className = 'cap';
      cap.style.setProperty('--i', i);
      cap.innerHTML = '<span class="lat"></span><span class="cyr"></span>';
      cap.firstChild.textContent = ch.toUpperCase();
      cap.lastChild.textContent = cyr(ch).toUpperCase();
      keysEl.appendChild(cap);
    });
  }

  function setVerdict(el, value) {
    if (value) el.dataset.verdict = value; else el.removeAttribute('data-verdict');
    $('em', el).textContent = value ? t(value === 'yes' ? 'demo.word' : 'demo.noword') : '';
  }
  function refreshVerdicts() {
    [readEn, readRu].forEach(function (el) {
      if (el && el.dataset.verdict) setVerdict(el, el.dataset.verdict);
    });
  }

  function showFinal(ex) {
    var right = ex.typed === 'en' ? 'ru' : 'en';
    buildCaps(ex.keys);
    keysEl.dataset.pick = right;
    setLayout(right, false);
    textEl.textContent = reading(ex.keys, right);
    field.dataset.state = 'ok';
    $('code', readEn).textContent = reading(ex.keys, 'en');
    $('code', readRu).textContent = reading(ex.keys, 'ru');
    setVerdict(readEn, right === 'en' ? 'yes' : 'no');
    setVerdict(readRu, right === 'ru' ? 'yes' : 'no');
  }

  async function playExample(ex) {
    var wrong = ex.typed;
    var right = wrong === 'en' ? 'ru' : 'en';
    var caps;
    var i;

    buildCaps(ex.keys);
    caps = $$('.cap', keysEl);
    setLayout(wrong, false);
    textEl.textContent = '';
    field.dataset.state = 'wrong';
    $('code', readEn).textContent = '';
    $('code', readRu).textContent = '';
    setVerdict(readEn, '');
    setVerdict(readRu, '');
    await sleep(500);

    // 1. The user types in the wrong layout.
    for (i = 1; i <= ex.keys.length; i++) {
      await whenVisible();
      caps[i - 1].classList.add('down');
      textEl.textContent = reading(ex.keys.slice(0, i), wrong);
      $('code', readEn).textContent = reading(ex.keys.slice(0, i), 'en');
      $('code', readRu).textContent = reading(ex.keys.slice(0, i), 'ru');
      await sleep(95);
      caps[i - 1].classList.remove('down');
      await sleep(70 + Math.random() * 90);
    }

    // 2. Space: both readings are compared.
    await sleep(320);
    field.dataset.state = 'check';
    await sleep(420);
    setVerdict(wrong === 'en' ? readEn : readRu, 'no');
    await sleep(260);
    setVerdict(right === 'en' ? readEn : readRu, 'yes');
    keysEl.dataset.pick = right;
    await sleep(520);

    // 3. The word is erased, the layout switched, the word typed again.
    for (i = ex.keys.length - 1; i >= 0; i--) {
      textEl.textContent = reading(ex.keys.slice(0, i), wrong);
      await sleep(32);
    }
    setLayout(right, true);
    await sleep(260);
    field.dataset.state = 'ok';
    for (i = 1; i <= ex.keys.length; i++) {
      textEl.textContent = reading(ex.keys.slice(0, i), right);
      await sleep(42);
    }
    await sleep(2100);
  }

  async function runDemo() {
    var n = 0;
    for (;;) {
      await whenVisible();
      await playExample(EXAMPLES[n % EXAMPLES.length]);
      n++;
    }
  }

  /* ------------------------------------------------------------------ *
   * Scroll motion
   * ------------------------------------------------------------------ */
  var bar = $('#progressBar');
  var header = $('#top');
  var blobs = $$('[data-parallax]');
  var rows = $$('[data-drift]');
  var ribbon = $('.ribbon');
  var shot = $('#heroShot');
  var steps = $('#steps');
  var tour = $('.tour-grid');
  var frame = $('#tourFrame');
  var ticking = false;

  function clamp(v) { return v < 0 ? 0 : v > 1 ? 1 : v; }

  function onFrame() {
    ticking = false;
    var y = window.scrollY;
    var vh = window.innerHeight;
    var max = doc.documentElement.scrollHeight - vh;
    var r;
    var p;

    bar.style.transform = 'scaleX(' + (max > 0 ? clamp(y / max) : 0) + ')';
    header.classList.toggle('is-stuck', y > 12);
    if (reduced) return;

    blobs.forEach(function (b) {
      b.style.transform = 'translate3d(0,' + (y * parseFloat(b.dataset.parallax)).toFixed(1) + 'px,0)';
    });

    r = ribbon.getBoundingClientRect();
    if (r.bottom > -100 && r.top < vh + 100) {
      rows.forEach(function (row) {
        row.style.transform = 'translate3d(' + ((vh - r.top) * parseFloat(row.dataset.drift)).toFixed(1) + 'px,0,0)';
      });
    }

    r = shot.getBoundingClientRect();
    if (r.bottom > 0 && r.top < vh) {
      p = clamp((vh - r.top) / (vh * 0.8));
      shot.style.transform = 'translateY(' + ((1 - p) * 50).toFixed(1) + 'px) rotateX(' + ((1 - p) * 26).toFixed(2) +
        'deg) scale(' + (0.88 + 0.12 * p).toFixed(3) + ')';
    }

    r = steps.getBoundingClientRect();
    steps.style.setProperty('--draw', clamp((vh * 0.8 - r.top) / (vh * 0.4)).toFixed(3));

    r = tour.getBoundingClientRect();
    if (r.bottom > 0 && r.top < vh) {
      p = clamp((vh * 0.5 - r.top) / Math.max(1, r.height - vh * 0.5));
      frame.style.transform = 'perspective(1400px) rotateY(' + ((0.5 - p) * 9).toFixed(2) + 'deg) rotateX(' +
        ((p - 0.5) * 4).toFixed(2) + 'deg)';
    }
  }
  function requestFrame() {
    if (!ticking) { ticking = true; requestAnimationFrame(onFrame); }
  }
  window.addEventListener('scroll', requestFrame, { passive: true });
  window.addEventListener('resize', requestFrame);

  /* Reveal on scroll, with staggered groups */
  $$('[data-stagger]').forEach(function (group) {
    var step = parseInt(group.dataset.stagger, 10) || 80;
    $$('.reveal', group).forEach(function (el, i) { el.style.setProperty('--d', i * step + 'ms'); });
  });
  if ('IntersectionObserver' in window && !reduced) {
    var revealer = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (e.isIntersecting) { e.target.classList.add('is-in'); revealer.unobserve(e.target); }
      });
    }, { threshold: 0.12, rootMargin: '0px 0px -6% 0px' });
    $$('.reveal').forEach(function (el) { revealer.observe(el); });
  } else {
    $$('.reveal').forEach(function (el) { el.classList.add('is-in'); });
  }

  /* Tour: the step crossing the middle of the screen picks the screenshot */
  var tourSteps = $$('.tour-step');
  var tourImgs = $$('img', frame);
  var tourDots = $$('#tourDots i');
  var tourIndex = 0;
  function setTour(index) {
    var prev = tourIndex;
    if (index < 0 || index === prev) return;
    tourIndex = index;
    tourSteps.forEach(function (el, i) { el.classList.toggle('is-active', i === index); });
    // Forward: the new screenshot comes in from the right and pushes the old one out to the left.
    // Backward the motion reverses. Screenshots in between jump without crossing the frame.
    tourImgs.forEach(function (el, i) {
      var moving = i === index || i === prev;
      if (!moving) el.style.transition = 'none';
      el.classList.toggle('is-active', i === index);
      el.classList.toggle('is-past', i < index);
      if (!moving) { void el.offsetWidth; el.style.transition = ''; }
    });
    tourDots.forEach(function (el, i) { el.classList.toggle('is-active', i === index); });
  }
  if ('IntersectionObserver' in window) {
    var tourWatch = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (e.isIntersecting) setTour(tourSteps.indexOf(e.target));
      });
    }, { rootMargin: '-48% 0px -48% 0px' });
    tourSteps.forEach(function (el) { tourWatch.observe(el); });

    /* Current section in the header */
    var links = $$('.nav a');
    var navWatch = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (!e.isIntersecting) return;
        links.forEach(function (a) {
          a.classList.toggle('is-current', a.getAttribute('href') === '#' + e.target.id);
        });
      });
    }, { rootMargin: '-40% 0px -55% 0px' });
    $$('main section[id]').forEach(function (s) { navWatch.observe(s); });

    /* The demo only runs while it is on screen */
    new IntersectionObserver(function (entries) {
      demoVisible = entries[0].isIntersecting;
    }, { threshold: 0.2 }).observe($('.demo'));
  } else {
    demoVisible = true;
  }

  /* Pointer glow on feature cards */
  $$('.card').forEach(function (card) {
    card.addEventListener('pointermove', function (e) {
      var r = card.getBoundingClientRect();
      card.style.setProperty('--mx', (e.clientX - r.left) + 'px');
      card.style.setProperty('--my', (e.clientY - r.top) + 'px');
    });
  });

  /* Copy buttons */
  $$('.copy').forEach(function (btn) {
    btn.addEventListener('click', function () {
      if (!navigator.clipboard) return;
      navigator.clipboard.writeText(btn.dataset.copy).then(function () {
        btn.textContent = t('copied');
        btn.classList.add('done');
        setTimeout(function () { btn.textContent = t('copy'); btn.classList.remove('done'); }, 1600);
      });
    });
  });

  /* ------------------------------------------------------------------ *
   * Newest release. GitHub Pages serves static files only, so the page asks
   * the GitHub API which release is the newest (prereleases included) and
   * rewrites the version and download links. Without network or with the API
   * limit reached, the version of the build stays.
   * ------------------------------------------------------------------ */
  var REPO = 'coinman-dev/own-keyboard-switch';
  var ASSETS = {
    portable: /portable\.exe$/i,
    installer: /install\.exe$/i,
    deb: /_amd64\.deb$/i,
    appimage: /\.AppImage$/i
  };

  function applyRelease(rel) {
    var next = String(rel.tag_name || '').replace(/^v/, '');
    if (!/^\d+\.\d+/.test(next)) return;
    var old = version;
    version = next;
    if (next !== old) {
      // Texts with the version: headings, file names, install commands.
      $$('[data-asset] .dl-file, .code code').forEach(function (el) {
        el.textContent = el.textContent.split(old).join(next);
      });
      $$('.copy').forEach(function (el) { el.dataset.copy = el.dataset.copy.split(old).join(next); });
      $$('[data-asset]').forEach(function (el) {
        el.setAttribute('href', el.getAttribute('href').split(old).join(next));
      });
      $$('[data-i18n]').forEach(function (el) {
        if (/\d+\.\d+/.test(DICT[lang][el.dataset.i18n] || RU[el.dataset.i18n] || '')) el.innerHTML = t(el.dataset.i18n);
      });
    }
    // Exact file links from the release. A file not attached yet (the Linux
    // files are uploaded after the Windows ones) points to the release page.
    $$('[data-asset]').forEach(function (el) {
      var asset = (rel.assets || []).filter(function (a) { return ASSETS[el.dataset.asset].test(a.name); })[0];
      if (asset) {
        el.setAttribute('href', asset.browser_download_url);
        $('.dl-file', el).textContent = asset.name;
      } else if (rel.html_url) {
        el.setAttribute('href', rel.html_url);
      }
    });
    $('#prereleaseNote').hidden = !rel.prerelease;
  }

  function loadRelease() {
    var cacheKey = 'okbs-release';
    try {
      var cached = JSON.parse(sessionStorage.getItem(cacheKey) || 'null');
      if (cached && Date.now() - cached.at < 30 * 60 * 1000) { applyRelease(cached.rel); return; }
    } catch (e) { /* storage unavailable */ }
    if (!window.fetch) return;
    fetch('https://api.github.com/repos/' + REPO + '/releases?per_page=5', {
      headers: { Accept: 'application/vnd.github+json' }
    }).then(function (r) { return r.ok ? r.json() : []; }).then(function (list) {
      var rel = list.filter(function (x) { return !x.draft; })[0];
      if (!rel) return;
      // Keep only what the page needs.
      rel = {
        tag_name: rel.tag_name,
        html_url: rel.html_url,
        prerelease: rel.prerelease,
        assets: (rel.assets || []).map(function (a) { return { name: a.name, browser_download_url: a.browser_download_url }; })
      };
      try { sessionStorage.setItem(cacheKey, JSON.stringify({ at: Date.now(), rel: rel })); } catch (e) { /* ignore */ }
      applyRelease(rel);
    }).catch(function () { /* offline: keep the built version */ });
  }

  /* ------------------------------------------------------------------ */
  applyLang(initialLang(), false);
  loadRelease();
  onFrame();
  if (reduced) showFinal(EXAMPLES[0]); else runDemo();
})();
