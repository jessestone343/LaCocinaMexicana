import './style.css';
import { renderMenu } from './menu-renderer.js';
import { initialContent, getPublishedContent, renderGallery } from './content-client.js';

// ========================================
//  LA COCINA MEXICANA — Interactions
// ========================================

document.addEventListener('DOMContentLoaded', async () => {

  // --- Banner Close logic ---
  const banner = document.getElementById('top-banner');
  const bannerClose = document.getElementById('close-banner');
  if (banner && bannerClose) {
    document.documentElement.style.scrollPaddingTop = (70 + banner.offsetHeight) + 'px';
    bannerClose.addEventListener('click', () => {
      banner.style.display = 'none';
      document.documentElement.style.scrollPaddingTop = '70px';
    });
  }

  // --- Mobile Menu Toggle ---
  const toggle = document.getElementById('nav-toggle');
  const menu   = document.getElementById('nav-menu');
  const links  = document.querySelectorAll('.nav-link');

  toggle.addEventListener('click', () => {
    const isOpen = menu.classList.toggle('open');
    toggle.classList.toggle('active');
    toggle.setAttribute('aria-expanded', isOpen);
  });

  // Close menu on normal link click (except dropdown toggle on mobile)
  links.forEach(link => {
    link.addEventListener('click', (e) => {
      // If it's the dropdown toggle on mobile, don't close the menu yet
      if (link.parentElement.classList.contains('nav-dropdown') && window.innerWidth < 1024) {
        e.preventDefault();
        link.parentElement.classList.toggle('expanded');
        return;
      }

      // Smooth scroll to target section
      const href = link.getAttribute('href');
      if (href && href.startsWith('#')) {
        e.preventDefault();
        let target = document.querySelector(href);
        
        if (target) {
          if (href === '#menu-lunch-specials') {
            const specialItem = target.querySelector('.highlight-special');
            if (specialItem) {
              target = specialItem;
            }
          }
          if (target.classList.contains('highlight-special')) {
            target.scrollIntoView({ behavior: 'smooth', block: 'center' });
          } else {
            target.scrollIntoView({ behavior: 'smooth' });
          }
        }
      }
      
      menu.classList.remove('open');
      toggle.classList.remove('active');
      toggle.setAttribute('aria-expanded', 'false');
    });
  });

  // Also close menu when clicking inside dropdown on mobile
  const dropdownLinks = document.querySelectorAll('.dropdown-link');
  dropdownLinks.forEach(link => {
    link.addEventListener('click', (e) => {
      // Smooth scroll to target section
      const href = link.getAttribute('href');
      if (href && href.startsWith('#')) {
        e.preventDefault();
        let target = document.querySelector(href);
        
        if (target) {
          if (href === '#menu-lunch-specials') {
            const specialItem = target.querySelector('.highlight-special');
            if (specialItem) {
              target = specialItem;
            }
          }
          if (target.classList.contains('highlight-special')) {
            target.scrollIntoView({ behavior: 'smooth', block: 'center' });
          } else {
            target.scrollIntoView({ behavior: 'smooth' });
          }
        }
      }

      if (window.innerWidth < 1024) {
        menu.classList.remove('open');
        toggle.classList.remove('active');
        toggle.setAttribute('aria-expanded', 'false');
      }
    });
  });

  // --- Scroll: Nav shadow + active link ---
  const nav      = document.getElementById('main-nav');
  const sections = document.querySelectorAll('.section, .hero, .footer');
  const sectionLinks = Array.from(links).filter(link => {
    const href = link.getAttribute('href');
    return href && document.querySelector(href);
  });

  const setActiveLink = (sectionId) => {
    sectionLinks.forEach(link => {
      link.classList.toggle('active', link.getAttribute('href') === `#${sectionId}`);
    });
  };

  const getCurrentSection = () => {
    if ((window.innerHeight + Math.ceil(window.scrollY)) >= document.body.offsetHeight - 20) {
      return 'contact';
    }

    const viewportAnchor = window.innerHeight * 0.38;
    let closestSection = sections[0];
    let closestDistance = Number.POSITIVE_INFINITY;

    sections.forEach(section => {
      const rect = section.getBoundingClientRect();
      if (rect.bottom <= 0 || rect.top >= window.innerHeight) return;

      const distance = Math.abs(rect.top - viewportAnchor);
      if (distance < closestDistance) {
        closestDistance = distance;
        closestSection = section;
      }
    });

    return closestSection ? closestSection.getAttribute('id') : '';
  };

  const onScroll = () => {
    // Nav shadow
    nav.classList.toggle('scrolled', window.scrollY > 20);

    setActiveLink(getCurrentSection());
  };

  window.addEventListener('scroll', onScroll, { passive: true });
  onScroll();

  // --- Fade-In on Scroll (Intersection Observer) ---
  const faders = document.querySelectorAll('.fade-in');
  let observer;

  if ('IntersectionObserver' in window) {
    observer = new IntersectionObserver((entries) => {
      entries.forEach(entry => {
        if (entry.isIntersecting) {
          entry.target.classList.add('visible');
          observer.unobserve(entry.target);
        }
      });
    }, {
      threshold: 0,
      rootMargin: '0px 0px -40px 0px'
    });

    faders.forEach(el => observer.observe(el));
  } else {
    // Fallback: just show everything
    faders.forEach(el => el.classList.add('visible'));
  }

  // Published content is shared with the business dashboard; seed data keeps the
  // restaurant usable when the content service is temporarily unavailable.
  let content = await initialContent();
  const menuContainer = document.getElementById('dynamic-menu-container');
  const galleryGrid = document.getElementById('gallery-grid');
  function showContent() {
    if (menuContainer) menuContainer.innerHTML = renderMenu(content.menu_items);
    if (galleryGrid) galleryGrid.innerHTML = renderGallery(content.gallery);
  }
  showContent();
  setInterval(async () => {
    if (document.hidden) return;
    try {
      const published = await getPublishedContent();
      if (JSON.stringify(published) !== JSON.stringify(content)) {
        content = published;
        showContent();
      }
    } catch { /* Keep the last successfully loaded menu and gallery. */ }
  }, 30000);

  // --- Google Reviews Loading ---
  const reviewsGrid = document.getElementById('reviews-grid');
  const reviewsRating = document.getElementById('reviews-rating');
  const reviewsCount = document.getElementById('reviews-count');
  const reviewsLink = document.getElementById('reviews-link');
  const reviewsDebugEnabled = new URLSearchParams(window.location.search).get('reviewsDebug') === '1';
  const reviewsMockEnabled = ['localhost', '127.0.0.1'].includes(window.location.hostname);

  const MOCK_REVIEWS_DATA = {
    source: 'Mock Google Business Profile',
    provider: 'local-mock',
    place: {
      name: 'La Cocina Mexicana',
      rating: 4.5,
      userRatingCount: 480,
      googleMapsUri: 'https://share.google/jdQG4F1zb468JzTCB'
    },
    reviews: [
      {
        author: 'Chris English',
        rating: 5,
        text: 'This place is adorable, cozy and the owner treats you like family, excellent value and authentic Mexican food. I highly recommend.',
        relativeTime: '6 months ago'
      },
      {
        author: 'Chel Martin',
        rating: 5,
        text: 'Great food. Perfect if you are craving Mexican food. Great prices for a family night out. Employees are friendly and helpful.',
        relativeTime: '2 months ago'
      },
      {
        author: 'Prapti Patel',
        rating: 5,
        text: 'We come once a year when here, and always plan to lunch here. Friendly, helpful, and prompt service. Delicious meal to boot.',
        relativeTime: '2 years ago'
      },
      {
        author: 'Megan R.',
        rating: 5,
        text: 'The food was delicious, the portions were huge, and our waiter was very polite and helpful. Many colorful details inside.',
        relativeTime: '8 months ago'
      },
      {
        author: 'Andrew S.',
        rating: 5,
        text: 'We had such a great experience here. The food was delicious and service was phenomenal. We will definitely be returning.',
        relativeTime: '1 year ago'
      }
    ]
  };

  const escapeHtml = (value = '') => String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');

  const formatReviewDate = (review) => {
    if (review.relativeTime) return review.relativeTime;
    if (!review.publishedAt) return 'Google review';

    try {
      return new Intl.DateTimeFormat('en-US', {
        month: 'short',
        year: 'numeric'
      }).format(new Date(review.publishedAt));
    } catch (_) {
      return 'Google review';
    }
  };

  const getReviewsDiagnosticsHtml = (data) => {
    if (!reviewsDebugEnabled || !data || !data.diagnostics) return '';

    const diagnostics = data.diagnostics;
    const env = diagnostics.env || {};
    const attempts = Array.isArray(diagnostics.attempts) ? diagnostics.attempts : [];

    const attemptRows = attempts.length
      ? attempts.map(attempt => `
          <li>
            <strong>${escapeHtml(attempt.provider)}:</strong>
            ${attempt.ok ? 'connected' : 'failed'}
            ${attempt.httpStatus ? `(HTTP ${escapeHtml(attempt.httpStatus)})` : ''}
            ${attempt.googleStatus ? `, Google status ${escapeHtml(attempt.googleStatus)}` : ''}
            ${attempt.error ? `, ${escapeHtml(attempt.error)}` : ''}
            <br>
            Reviews returned: ${escapeHtml(attempt.rawReviewCount ?? 'n/a')};
            with text: ${escapeHtml(attempt.textReviewCount ?? 'n/a')};
            5-star with text: ${escapeHtml(attempt.fiveStarTextReviewCount ?? 'n/a')};
            ratings: ${escapeHtml((attempt.ratingsReturned || []).join(', ') || 'none')}
          </li>
        `).join('')
      : '<li>No Google request was attempted.</li>';

    return `
      <div class="reviews-debug">
        <strong>Review diagnostics</strong>
        <p>
          API key present: ${env.apiKeyPresent ? 'yes' : 'no'};
          key env: ${escapeHtml(env.apiKeyEnvName || 'missing')};
          Place ID present: ${env.placeIdPresent ? 'yes' : 'no'};
          Place ID length: ${escapeHtml(env.placeIdLength || 0)};
          limit: ${escapeHtml(env.reviewLimit || '')}
        </p>
        <ul>${attemptRows}</ul>
      </div>
    `;
  };

  const showReviewsUnavailable = (data) => {
    if (!reviewsGrid || !reviewsCount) return;

    reviewsCount.textContent = 'Google reviews are available on the restaurant listing.';
    reviewsGrid.innerHTML = `
      <div class="review-card">
        <p class="reviews-empty">
          ${escapeHtml((data && data.message) || 'Live Google reviews are temporarily unavailable. Guests can still read the latest reviews directly on Google.')}
        </p>
        ${getReviewsDiagnosticsHtml(data)}
      </div>
    `;
  };

  const renderReviews = (data) => {
    if (!reviewsGrid || !reviewsCount) return;

    const reviews = Array.isArray(data.reviews) ? data.reviews : [];
    if (!reviews.length) {
      showReviewsUnavailable(data);
      return;
    }

    if (reviewsRating && data.place && data.place.rating) {
      reviewsRating.textContent = Number(data.place.rating).toFixed(1);
    }

    if (reviewsLink && data.place && data.place.googleMapsUri) {
      reviewsLink.href = data.place.googleMapsUri;
    }

    const totalReviews = data.place && data.place.userRatingCount
      ? `${data.place.userRatingCount.toLocaleString()} Google reviews`
      : `${reviews.length} featured 5-star reviews`;
    reviewsCount.textContent = totalReviews;

    const reviewCards = reviews.map(review => `
      <article class="review-card">
        <div class="review-stars" aria-label="5 out of 5 stars">★★★★★</div>
        <p class="review-text">"${escapeHtml(review.text)}"</p>
        <div class="review-footer">
          <span class="review-author">${escapeHtml(review.author || 'Google reviewer')}</span>
          <span class="review-date">${escapeHtml(formatReviewDate(review))}</span>
        </div>
      </article>
    `);

    const googleReviewsUrl = (data.place && data.place.googleMapsUri)
      || (reviewsLink && reviewsLink.href)
      || 'https://share.google/jdQG4F1zb468JzTCB';

    const ctaCard = reviews.length < 6 ? `
      <article class="review-card review-card-cta">
        <span class="review-cta-label">More from guests</span>
        <h3 class="review-cta-title">See the latest Google reviews</h3>
        <p class="review-cta-text">Google shares a small set of reviews through Places. The full, current list is always available on the restaurant's Google listing.</p>
        <a class="reviews-link" href="${escapeHtml(googleReviewsUrl)}" target="_blank" rel="noopener">
          Read more on Google
        </a>
      </article>
    ` : '';

    reviewsGrid.innerHTML = [...reviewCards, ctaCard].join('');
  };

  if (reviewsGrid && reviewsMockEnabled) {
    renderReviews(MOCK_REVIEWS_DATA);
  } else if (reviewsGrid) {
    fetch(`/api/google-reviews${reviewsDebugEnabled ? '?debug=1' : ''}`)
      .then(response => {
        return response.json();
      })
      .then(renderReviews)
      .catch(() => showReviewsUnavailable());
  }

  // --- Mobile Vegetarian Tooltip ---
  document.addEventListener('click', (e) => {
    // Remove existing tooltip if clicked anywhere else
    const existing = document.querySelector('.veg-tooltip');
    if (existing && !e.target.classList.contains('veg-indicator')) {
      existing.remove();
    }

    if (e.target.classList.contains('veg-indicator')) {
      e.preventDefault();
      e.stopPropagation();
      
      let isSame = false;
      if (existing) {
        isSame = (existing.sourceIndicator === e.target);
        existing.remove(); 
      }

      if (isSame) return;

      const tooltip = document.createElement('div');
      tooltip.className = 'veg-tooltip';
      tooltip.innerText = 'Vegetarian';
      tooltip.sourceIndicator = e.target;
      
      const rect = e.target.getBoundingClientRect();
      tooltip.style.left = `${rect.left + (rect.width / 2) + window.scrollX}px`;
      tooltip.style.top = `${rect.top + window.scrollY - 20}px`;
      
      document.body.appendChild(tooltip);
      
      setTimeout(() => {
        if (tooltip.parentElement) tooltip.remove();
      }, 2500);
    }
  });

  // --- Gallery Lightbox ---
  const lightbox = document.getElementById('lightbox');
  const lightboxImg = document.getElementById('lightbox-img');
  const lightboxClose = document.getElementById('lightbox-close');
  const lightboxPrev = document.getElementById('lightbox-prev');
  const lightboxNext = document.getElementById('lightbox-next');
  
  let currentImageIndex = 0;
  const images = () => Array.from(document.querySelectorAll('.gallery-item img'));

  function openLightbox(index) {
    if (!lightbox) return;
    currentImageIndex = index;
    lightboxImg.src = images()[currentImageIndex]?.src || '';
    lightboxImg.alt = images()[currentImageIndex]?.alt || '';
    lightbox.classList.add('active');
    document.body.style.overflow = 'hidden'; // Prevent background scrolling
  }

  function closeLightbox() {
    if (!lightbox) return;
    lightbox.classList.remove('active');
    document.body.style.overflow = '';
  }

  function showPrevImage() {
    currentImageIndex = (currentImageIndex - 1 + images().length) % images().length;
    lightboxImg.src = images()[currentImageIndex]?.src || '';
    lightboxImg.alt = images()[currentImageIndex]?.alt || '';
  }

  function showNextImage() {
    currentImageIndex = (currentImageIndex + 1) % images().length;
    lightboxImg.src = images()[currentImageIndex]?.src || '';
    lightboxImg.alt = images()[currentImageIndex]?.alt || '';
  }

  if (lightbox) {
    // Delegate so photos published by the dashboard also open in the lightbox.
    galleryGrid?.addEventListener('click', (event) => {
      const image = event.target.closest('.gallery-item img');
      if (image) openLightbox(images().indexOf(image));
    });

    // Close buttons
    lightboxClose.addEventListener('click', closeLightbox);
    lightbox.addEventListener('click', (e) => {
      if (e.target === lightbox) closeLightbox();
    });

    // Navigation buttons
    lightboxPrev.addEventListener('click', (e) => {
      e.stopPropagation();
      showPrevImage();
    });
    
    lightboxNext.addEventListener('click', (e) => {
      e.stopPropagation();
      showNextImage();
    });

    // Keyboard navigation
    document.addEventListener('keydown', (e) => {
      if (!lightbox.classList.contains('active')) return;
      if (e.key === 'Escape') closeLightbox();
      if (e.key === 'ArrowLeft') showPrevImage();
      if (e.key === 'ArrowRight') showNextImage();
    });

    // Touch Support (Swipe)
    let touchStartX = 0;
    let touchEndX = 0;

    lightbox.addEventListener('touchstart', e => {
      touchStartX = e.changedTouches[0].screenX;
    }, {passive: true});

    lightbox.addEventListener('touchend', e => {
      touchEndX = e.changedTouches[0].screenX;
      handleSwipe();
    }, {passive: true});

    function handleSwipe() {
      const swipeThreshold = 50;
      if (touchEndX < touchStartX - swipeThreshold) {
        showNextImage(); // Swiped left
      }
      if (touchEndX > touchStartX + swipeThreshold) {
        showPrevImage(); // Swiped right
      }
    }
  }

});
