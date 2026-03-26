import http from 'k6/http';
import { check } from 'k6';
import encoding from 'k6/encoding';

const tokenPool = Array.from({ length: 9000 }, (_, i) => {
  const userId = 1000 + i;
  const token = generateFakeToken(userId)

  return `Bearer ${token}`;
});

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
  const token = tokenPool[Math.floor(Math.random() * tokenPool.length)];

  const url = 'http://127.0.0.1:8000/consume';

  const params = {
    headers: {
      'x-forwarded-host': 'that-limit.com',
      'authorization': token,
    },
  };

  const res = http.post(url, null, params);

  check(res, {
    'is status 200 or 429': (r) => r.status === 200 || r.status === 429,
  });
}

function generateFakeToken(userId) {
  const header = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
  const payload = encoding.b64encode(JSON.stringify({
    sub: userId.toString(),
  }), 'rawurl');

  return `${header}.${payload}.`;
}
