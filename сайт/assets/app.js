(() => {
  const $ = (s, r = document) => r.querySelector(s), $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const nav = $('nav.top'); const onScroll = () => nav && nav.classList.toggle('s', scrollY > 20); addEventListener('scroll', onScroll, {passive: true}); onScroll();
  // появление блоков при прокрутке
  const io = new IntersectionObserver(es => es.forEach(e => { if (e.isIntersecting) { e.target.classList.add('in'); io.unobserve(e.target); } }), {threshold: .12});
  $$('.rv').forEach(el => io.observe(el));
  // подсветка карточек под курсором
  $$('.card').forEach(c => c.addEventListener('mousemove', e => { const r = c.getBoundingClientRect(); c.style.setProperty('--mx', e.clientX - r.left + 'px'); c.style.setProperty('--my', e.clientY - r.top + 'px'); }));
  // наклон скриншота в шапке
  const hs = $('.hero .shot'); if (hs) { addEventListener('mousemove', e => { const x = e.clientX / innerWidth - .5, y = e.clientY / innerHeight - .5; hs.style.setProperty('--ry', (x * 6).toFixed(2) + 'deg'); hs.style.setProperty('--rx', (6 - y * 5).toFixed(2) + 'deg'); }); }
  // вкладки с экранами
  const caps = {home: 'Профили слева, у каждого своя папка с модами и мирами. Один клик: играть.', versions: 'Все версии Minecraft и шесть загрузчиков. Сборка Noxel ставит набор модов сразу.', catalog: 'Modrinth и CurseForge в одном поиске. Видно, на какой версии мод уже скачан.', drawer: 'Описание, версии с обоих сайтов (релиз, бета, альфа) и установка нужной, не выходя из лаунчера.', servers: 'Рекомендуемые моды для популярных серверов. Список хранится на сервере Noxel и обновляется сам.', accounts: 'Объёмная анимированная модель скина, история скинов, время игры, серверы и миры.', settings: 'Память, сборщик мусора, Java, окно игры, поведение лаунчера и анимации.'};
  $$('.tabs button').forEach(b => b.addEventListener('click', () => {
    $$('.tabs button').forEach(x => x.classList.toggle('on', x === b));
    const img = $('#demoImg'); img.style.opacity = 0; setTimeout(() => { img.src = 'assets/screens/' + b.dataset.s + '.webp'; img.onload = () => img.style.opacity = 1; }, 220);
    $('#demoCap').textContent = caps[b.dataset.s];
  }));
  // копирование команд
  $$('[data-copy]').forEach(b => b.addEventListener('click', () => { navigator.clipboard.writeText($(b.dataset.copy).textContent.trim()).then(() => { const t = b.textContent; b.textContent = 'Скопировано'; setTimeout(() => b.textContent = t, 1400); }); }));
  // скачивание: данные берутся с сервера Noxel
  const os = /Windows/i.test(navigator.userAgent) ? 'windows' : 'linux';
  document.documentElement.dataset.os = os;
  const fmt = n => (n / 1e6).toFixed(1).replace('.', ',') + ' МБ';
  Promise.all([fetch('/noxel/dl/downloads.json').then(r => r.json()).catch(() => ({})), fetch('/noxel/v1/launcher').then(r => r.json()).catch(() => ({}))]).then(([dl, lm]) => {
    const ver = lm.version || '';
    $$('[data-ver]').forEach(e => e.textContent = ver ? 'Версия ' + ver : '');
    const lin = (dl.installers || {})['linux-x86_64'], win = (dl.installers || {})['windows-x86_64'], bin = lm.files && lm.files['linux-x86_64'];
    $$('[data-lin]').forEach(a => { if (lin) { a.href = '/noxel/dl/' + lin.file; a.removeAttribute('aria-disabled'); } });
    $$('[data-win]').forEach(a => { if (win) { a.href = '/noxel/dl/' + win.file; a.removeAttribute('aria-disabled'); a.textContent = a.dataset.label || a.textContent; } });
    $$('[data-lin-size]').forEach(e => e.textContent = lin ? fmt(lin.size) : '');
    $$('[data-lin-sha]').forEach(e => e.textContent = lin ? lin.sha256 : '');
    $$('[data-bin]').forEach(a => { if (bin) { a.href = bin.url; a.removeAttribute('aria-disabled'); } });
    $$('[data-bin-sha]').forEach(e => e.textContent = bin ? bin.sha256 : '');
    $$('[data-meta]').forEach(e => e.textContent = [ver && 'Версия ' + ver, lin && fmt(lin.size), 'бесплатно, без рекламы'].filter(Boolean).join(' · '));
    if (win) $$('[data-win-soon]').forEach(e => e.style.display = 'none');
  });
  // оглавление в документации
  const links = $$('.toc a'); if (links.length) { const secs = links.map(a => $(a.getAttribute('href'))).filter(Boolean);
    const so = new IntersectionObserver(es => es.forEach(e => { if (e.isIntersecting) links.forEach(a => a.classList.toggle('on', a.getAttribute('href') === '#' + e.target.id)); }), {rootMargin: '-20% 0px -70% 0px'}); secs.forEach(s => so.observe(s)); }
})();
