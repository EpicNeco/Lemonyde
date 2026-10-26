// Smooth scroll with offset for fixed header
document.querySelectorAll('a[href^="#"]').forEach(anchor => {
  anchor.addEventListener('click', function (e) {
    const href = this.getAttribute('href');
    if (href === '#' || href === '#top') {
      e.preventDefault();
      window.scrollTo({ top: 0, behavior: 'smooth' });
      return;
    }
    const target = document.querySelector(href);
    if (target) {
      e.preventDefault();
      const headerHeight = document.querySelector('.site-header').offsetHeight;
      const targetPosition = target.getBoundingClientRect().top + window.pageYOffset - headerHeight - 20;
      window.scrollTo({ top: targetPosition, behavior: 'smooth' });
    }
  });
});

// Header background on scroll
const header = document.querySelector('.site-header');
let lastScroll = 0;

window.addEventListener('scroll', () => {
  const currentScroll = window.pageYOffset;
  
  if (currentScroll > 50) {
    header.style.background = 'rgba(10, 11, 16, 0.95)';
    header.style.boxShadow = '0 1px 3px rgba(0, 0, 0, 0.3)';
  } else {
    header.style.background = 'rgba(10, 11, 16, 0.8)';
    header.style.boxShadow = 'none';
  }
  
  lastScroll = currentScroll;
});

// Add subtle parallax to hero gradient
window.addEventListener('scroll', () => {
  const scrolled = window.pageYOffset;
  const hero = document.querySelector('.hero');
  if (hero && scrolled < 800) {
    hero.style.backgroundPosition = `50% ${scrolled * 0.5}px`;
  }
});

// Copy install command on click
const codeBlock = document.querySelector('.code-block code');
if (codeBlock) {
  codeBlock.style.cursor = 'pointer';
  codeBlock.title = 'Click to copy';
  
  codeBlock.addEventListener('click', async () => {
    const text = codeBlock.textContent;
    try {
      await navigator.clipboard.writeText(text);
      const original = codeBlock.textContent;
      codeBlock.textContent = 'Copied to clipboard!';
      codeBlock.style.color = '#22c55e';
      setTimeout(() => {
        codeBlock.textContent = original;
        codeBlock.style.color = 'var(--accent)';
      }, 2000);
    } catch (err) {
      // Fallback for older browsers
      const textarea = document.createElement('textarea');
      textarea.value = text;
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand('copy');
      document.body.removeChild(textarea);
      const original = codeBlock.textContent;
      codeBlock.textContent = 'Copied!';
      setTimeout(() => {
        codeBlock.textContent = original;
      }, 1500);
    }
  });
}

// Intersection Observer for fade-in animations
const observerOptions = {
  threshold: 0.1,
  rootMargin: '0px 0px -50px 0px'
};

const observer = new IntersectionObserver((entries) => {
  entries.forEach(entry => {
    if (entry.isIntersecting) {
      entry.target.style.opacity = '1';
      entry.target.style.transform = 'translateY(0)';
    }
  });
}, observerOptions);

document.querySelectorAll('.feature-card').forEach((card, index) => {
  card.style.opacity = '0';
  card.style.transform = 'translateY(30px)';
  card.style.transition = `all 0.6s ease ${index * 0.1}s`;
  observer.observe(card);
});
