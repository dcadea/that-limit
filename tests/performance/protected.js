import http from 'k6/http';
import { check } from 'k6';

export const options = {
  stages: [
    { duration: '30s', target: 100 },
    { duration: '1m', target: 100 },
    { duration: '30s', target: 0 },
  ],
  thresholds: {
    http_req_failed: ['rate<0.01'],
    http_req_duration: ['p(99)<10'],
  },
};

export default function () {
  const userId = randomIntBetween(1000, 9999);

  const url = 'http://127.0.0.1:8000/consume';

  const params = {
    headers: {
      'user_id': userId.toString(),
    },
  };

  const res = http.post(url, null, params);

  check(res, {
    'is status 200 or 429': (r) => r.status === 200 || r.status === 429,
  });
}

function randomIntBetween(min, max) {
  return Math.floor(Math.random() * (max - min + 1) + min);
}
