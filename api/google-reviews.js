const DEFAULT_REVIEW_LIMIT = 3;
const MAX_REVIEW_LIMIT = 5;
const GOOGLE_MAPS_URL = 'https://share.google/jdQG4F1zb468JzTCB';

function sendJson(response, statusCode, payload, cacheControl) {
  response.statusCode = statusCode;
  response.setHeader('Content-Type', 'application/json; charset=utf-8');
  response.setHeader('Cache-Control', cacheControl || 'public, s-maxage=21600, stale-while-revalidate=86400');
  response.end(JSON.stringify(payload));
}

function getRequestUrl(request) {
  return new URL(request.url, `https://${request.headers.host || 'localhost'}`);
}

function getReviewLimit() {
  const requestedLimit = Number(process.env.GOOGLE_REVIEWS_LIMIT || DEFAULT_REVIEW_LIMIT);
  return Math.min(Math.max(requestedLimit || DEFAULT_REVIEW_LIMIT, 1), MAX_REVIEW_LIMIT);
}

function getConfig() {
  return {
    apiKey: process.env.GOOGLE_PLACES_API_KEY || process.env.GOOGLE_MAPS_API_KEY || '',
    apiKeyEnvName: process.env.GOOGLE_PLACES_API_KEY
      ? 'GOOGLE_PLACES_API_KEY'
      : process.env.GOOGLE_MAPS_API_KEY
        ? 'GOOGLE_MAPS_API_KEY'
        : null,
    placeId: process.env.GOOGLE_PLACE_ID || '',
    reviewLimit: getReviewLimit()
  };
}

function getGoogleError(payload, fallbackStatus) {
  if (!payload) return `Google request failed with status ${fallbackStatus}`;
  if (payload.error && payload.error.message) return payload.error.message;
  if (payload.error_message) return payload.error_message;
  if (payload.status && payload.status !== 'OK') return payload.status;
  return `Google request failed with status ${fallbackStatus}`;
}

function normalizeNewReview(review) {
  const text = review && review.text && (review.text.text || review.text);
  const author = review && review.authorAttribution && review.authorAttribution.displayName;
  const authorUri = review && review.authorAttribution && review.authorAttribution.uri;

  return {
    author: author || 'Google reviewer',
    authorUri: authorUri || '',
    rating: Number(review.rating || 0),
    text: text || '',
    relativeTime: review.relativePublishTimeDescription || '',
    publishedAt: review.publishTime || '',
    googleMapsUri: review.googleMapsUri || ''
  };
}

function normalizeLegacyReview(review) {
  return {
    author: review.author_name || 'Google reviewer',
    authorUri: review.author_url || '',
    rating: Number(review.rating || 0),
    text: review.text || '',
    relativeTime: review.relative_time_description || '',
    publishedAt: review.time ? new Date(review.time * 1000).toISOString() : '',
    googleMapsUri: ''
  };
}

function summarizeReviews(rawReviews, normalizedReviews, filteredReviews) {
  return {
    rawReviewCount: rawReviews.length,
    textReviewCount: normalizedReviews.filter(review => review.text).length,
    fiveStarReviewCount: normalizedReviews.filter(review => review.rating === 5).length,
    fiveStarTextReviewCount: filteredReviews.length,
    ratingsReturned: normalizedReviews.map(review => review.rating),
    returnedReviews: normalizedReviews.map(review => ({
      author: review.author,
      rating: review.rating,
      hasText: Boolean(review.text),
      textLength: review.text.length,
      relativeTime: review.relativeTime || ''
    }))
  };
}

function filterReviews(rawReviews, normalizeReview, reviewLimit) {
  const normalizedReviews = rawReviews.map(normalizeReview);
  const filteredReviews = normalizedReviews
    .filter(review => review.rating === 5 && review.text)
    .slice(0, reviewLimit);

  return {
    reviews: filteredReviews,
    diagnostics: summarizeReviews(rawReviews, normalizedReviews, filteredReviews)
  };
}

async function fetchJson(url, options) {
  const googleResponse = await fetch(url, options);
  const body = await googleResponse.text();
  let payload = null;

  try {
    payload = body ? JSON.parse(body) : null;
  } catch (_) {
    payload = { rawBody: body.slice(0, 500) };
  }

  return { ok: googleResponse.ok, status: googleResponse.status, payload };
}

async function getPlaceDetailsNew(apiKey, placeId, reviewLimit) {
  const provider = 'places-api-new';
  const fields = [
    'id',
    'displayName',
    'rating',
    'userRatingCount',
    'googleMapsUri',
    'reviews'
  ].join(',');

  const url = new URL(`https://places.googleapis.com/v1/places/${encodeURIComponent(placeId)}`);
  url.searchParams.set('languageCode', 'en');
  url.searchParams.set('regionCode', 'US');

  const result = await fetchJson(url, {
    headers: {
      'X-Goog-Api-Key': apiKey,
      'X-Goog-FieldMask': fields
    }
  });

  if (!result.ok) {
    return {
      provider,
      ok: false,
      httpStatus: result.status,
      error: getGoogleError(result.payload, result.status),
      googleError: result.payload && result.payload.error ? result.payload.error : null
    };
  }

  const rawReviews = result.payload.reviews || [];
  const filtered = filterReviews(rawReviews, normalizeNewReview, reviewLimit);

  return {
    provider,
    ok: true,
    httpStatus: result.status,
    place: {
      id: result.payload.id || placeId,
      name: result.payload.displayName && result.payload.displayName.text
        ? result.payload.displayName.text
        : 'La Cocina Mexicana',
      rating: result.payload.rating || null,
      userRatingCount: result.payload.userRatingCount || null,
      googleMapsUri: result.payload.googleMapsUri || GOOGLE_MAPS_URL
    },
    reviews: filtered.reviews,
    diagnostics: filtered.diagnostics
  };
}

async function getPlaceDetailsLegacy(apiKey, placeId, reviewLimit) {
  const provider = 'places-api-legacy';
  const url = new URL('https://maps.googleapis.com/maps/api/place/details/json');
  url.searchParams.set('place_id', placeId);
  url.searchParams.set('fields', 'name,rating,user_ratings_total,url,reviews');
  url.searchParams.set('reviews_sort', 'newest');
  url.searchParams.set('language', 'en');
  url.searchParams.set('key', apiKey);

  const result = await fetchJson(url);
  if (!result.ok || !result.payload || result.payload.status !== 'OK') {
    return {
      provider,
      ok: false,
      httpStatus: result.status,
      googleStatus: result.payload && result.payload.status ? result.payload.status : null,
      error: getGoogleError(result.payload, result.status)
    };
  }

  const place = result.payload.result || {};
  const rawReviews = place.reviews || [];
  const filtered = filterReviews(rawReviews, normalizeLegacyReview, reviewLimit);

  return {
    provider,
    ok: true,
    httpStatus: result.status,
    googleStatus: result.payload.status,
    place: {
      id: placeId,
      name: place.name || 'La Cocina Mexicana',
      rating: place.rating || null,
      userRatingCount: place.user_ratings_total || null,
      googleMapsUri: place.url || GOOGLE_MAPS_URL
    },
    reviews: filtered.reviews,
    diagnostics: filtered.diagnostics
  };
}

function getUnavailableMessage(attempts) {
  const successfulAttempt = attempts.find(attempt => attempt.ok);
  if (successfulAttempt) {
    const diagnostics = successfulAttempt.diagnostics;
    if (!diagnostics.rawReviewCount) {
      return 'Google returned the business listing but did not include any reviews in the Places response.';
    }
    if (!diagnostics.fiveStarTextReviewCount) {
      return `Google returned ${diagnostics.rawReviewCount} review(s), but none matched the current 5-star review filter with written text.`;
    }
  }

  const failedAttempts = attempts.filter(attempt => !attempt.ok);
  if (failedAttempts.length) {
    return `Google rejected the Places request: ${failedAttempts.map(attempt => `${attempt.provider}: ${attempt.error}`).join(' | ')}`;
  }

  return 'Google reviews are temporarily unavailable.';
}

function getPublicDiagnostics(config, attempts) {
  return {
    env: {
      apiKeyPresent: Boolean(config.apiKey),
      apiKeyEnvName: config.apiKeyEnvName,
      placeIdPresent: Boolean(config.placeId),
      placeIdLength: config.placeId ? config.placeId.length : 0,
      reviewLimit: config.reviewLimit
    },
    attempts: attempts.map(attempt => ({
      provider: attempt.provider,
      ok: attempt.ok,
      httpStatus: attempt.httpStatus,
      googleStatus: attempt.googleStatus || null,
      error: attempt.error || null,
      rawReviewCount: attempt.diagnostics ? attempt.diagnostics.rawReviewCount : null,
      textReviewCount: attempt.diagnostics ? attempt.diagnostics.textReviewCount : null,
      fiveStarReviewCount: attempt.diagnostics ? attempt.diagnostics.fiveStarReviewCount : null,
      fiveStarTextReviewCount: attempt.diagnostics ? attempt.diagnostics.fiveStarTextReviewCount : null,
      ratingsReturned: attempt.diagnostics ? attempt.diagnostics.ratingsReturned : [],
      returnedReviews: attempt.diagnostics ? attempt.diagnostics.returnedReviews : []
    }))
  };
}

module.exports = async function handler(request, response) {
  if (request.method !== 'GET') {
    response.setHeader('Allow', 'GET');
    sendJson(response, 405, { error: 'Method not allowed' }, 'no-store');
    return;
  }

  const requestUrl = getRequestUrl(request);
  const debug = requestUrl.searchParams.get('debug') === '1';
  const config = getConfig();

  if (!config.apiKey || !config.placeId) {
    sendJson(response, debug ? 503 : 200, {
      source: 'Google Business Profile',
      configured: false,
      message: 'Google Places API environment variables are missing.',
      requiredEnv: ['GOOGLE_PLACES_API_KEY', 'GOOGLE_PLACE_ID'],
      diagnostics: getPublicDiagnostics(config, []),
      place: {
        name: 'La Cocina Mexicana',
        googleMapsUri: GOOGLE_MAPS_URL
      },
      reviews: []
    }, 'no-store');
    return;
  }

  const attempts = [];

  for (const getDetails of [getPlaceDetailsNew, getPlaceDetailsLegacy]) {
    const attempt = await getDetails(config.apiKey, config.placeId, config.reviewLimit);
    attempts.push(attempt);

    if (attempt.ok && attempt.reviews.length) {
      sendJson(response, 200, {
        source: 'Google Business Profile',
        configured: true,
        provider: attempt.provider,
        place: attempt.place,
        reviews: attempt.reviews,
        diagnostics: getPublicDiagnostics(config, attempts)
      });
      return;
    }
  }

  const successfulAttempt = attempts.find(attempt => attempt.ok);

  sendJson(response, debug ? 502 : 200, {
    source: 'Google Business Profile',
    configured: true,
    unavailable: true,
    message: getUnavailableMessage(attempts),
    place: successfulAttempt && successfulAttempt.place
      ? successfulAttempt.place
      : {
        name: 'La Cocina Mexicana',
        googleMapsUri: GOOGLE_MAPS_URL
      },
    reviews: [],
    diagnostics: getPublicDiagnostics(config, attempts)
  }, 'no-store');
};
