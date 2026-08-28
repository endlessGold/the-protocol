# 10-01 - HTTP REST API 설계 (미구현)

## 개요

The Protocol은 Axum 기반 HTTP REST API를 통해 브라우저, 모바일 앱, 외부 시스템과의 통합을 지원한다. 현재 TCP 프로토콜 중심이나, 향후 REST API가 추가될 예정이다.

## API 아키텍처

```
┌──────────────────────────────────────────────────────────┐
│                      클라이언트                           │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  │
│  │  브라우저  │  │  모바일   │  │  외부 시스템 (API)   │  │
│  └────┬─────┘  └────┬─────┘  └──────────┬───────────┘  │
└───────┼──────────────┼───────────────────┼───────────────┘
        │              │                   │
        │    HTTPS     │    HTTPS          │  HTTPS
        │              │                   │
┌───────▼──────────────▼───────────────────▼───────────────┐
│                    Axum HTTP Server                       │
│                                                           │
│  ┌─────────────────────────────────────────────────────┐ │
│  │                   Middleware Stack                   │ │
│  │  1. CORS                                           │ │
│  │  2. Tower HTTP (Timeout, Limit)                     │ │
│  │  3. Request Logging                                │ │
│  │  4. Rate Limiting                                  │ │
│  │  5. JWT Authentication                             │ │
│  └──────────────────────┬──────────────────────────────┘ │
│                         │                                 │
│  ┌──────────────────────▼──────────────────────────────┐ │
│  │                   Router                            │ │
│  │  /api/v1/auth/*                                     │ │
│  │  /api/v1/characters/*                               │ │
│  │  /api/v1/inventory/*                                │ │
│  │  /api/v1/auction/*                                  │ │
│  │  /api/v1/ranking                                    │ │
│  └──────────────────────┬──────────────────────────────┘ │
│                         │                                 │
│  ┌──────────────────────▼──────────────────────────────┐ │
│  │              Application Layer                      │ │
│  │  (GameWorld, AuthManager, RoleManager)              │ │
│  └──────────────────────┬──────────────────────────────┘ │
│                         │                                 │
│  ┌──────────────────────▼──────────────────────────────┐ │
│  │              Repository Layer                       │ │
│  │  (CharacterRepo, AccountRepo, InventoryRepo)        │ │
│  └─────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

## 엔드포인트 목록

### 인증 (Auth)

#### POST /api/v1/auth/login

로그인하여 JWT 토큰을 발급받는다.

**요청:**
```json
{
  "username": "player1",
  "password": "secure_password"
}
```

**응답 (200 OK):**
```json
{
  "success": true,
  "access_token": "eyJhbGciOiJIUzI1NiJ9...",
  "refresh_token": "eyJhbGciOiJIUzI1NiJ9...",
  "expires_in": 3600,
  "player": {
    "id": 1,
    "username": "player1",
    "role": "player"
  }
}
```

**에러 응답 (401 Unauthorized):**
```json
{
  "success": false,
  "error": {
    "code": "INVALID_CREDENTIALS",
    "message": "Invalid username or password"
  }
}
```

#### POST /api/v1/auth/register

새 계정을 등록한다.

**요청:**
```json
{
  "username": "newplayer",
  "email": "player@example.com",
  "password": "secure_password"
}
```

**응답 (201 Created):**
```json
{
  "success": true,
  "player": {
    "id": 2,
    "username": "newplayer",
    "email": "player@example.com",
    "role": "player",
    "created_at": "2026-08-28T10:00:00Z"
  }
}
```

#### POST /api/v1/auth/refresh

Refresh Token으로 새 Access Token을 발급받는다.

**요청:**
```json
{
  "refresh_token": "eyJhbGciOiJIUzI1NiJ9..."
}
```

**응답 (200 OK):**
```json
{
  "success": true,
  "access_token": "eyJhbGciOiJIUzI1NiJ9...",
  "refresh_token": "eyJhbGciOiJIUzI1NiJ9...",
  "expires_in": 3600
}
```

### 캐릭터 (Characters)

#### GET /api/v1/characters/:id

특정 캐릭터의 상세 정보를 조회한다.

**요청 헤더:**
```
Authorization: Bearer <access_token>
```

**응답 (200 OK):**
```json
{
  "success": true,
  "character": {
    "id": 1,
    "account_id": 1,
    "name": "Hero",
    "class": "Warrior",
    "level": 5,
    "experience": 4500,
    "hp": 120,
    "max_hp": 120,
    "mp": 28,
    "max_mp": 28,
    "stats": {
      "strength": 15,
      "dexterity": 10,
      "intelligence": 8,
      "wisdom": 8,
      "constitution": 14
    },
    "room_id": 3,
    "gold": 1500,
    "created_at": "2026-08-01T10:00:00Z",
    "updated_at": "2026-08-28T10:00:00Z"
  }
}
```

#### GET /api/v1/characters?account_id=:id

특정 계정의 모든 캐릭터를 조회한다.

**요청 헤더:**
```
Authorization: Bearer <access_token>
```

**응답 (200 OK):**
```json
{
  "success": true,
  "characters": [
    {
      "id": 1,
      "name": "Hero",
      "class": "Warrior",
      "level": 5,
      "hp": 120,
      "max_hp": 120,
      "room_id": 3
    },
    {
      "id": 2,
      "name": "Mage",
      "class": "Mage",
      "level": 3,
      "hp": 60,
      "max_hp": 60,
      "room_id": 1
    }
  ]
}
```

#### POST /api/v1/characters

새 캐릭터를 생성한다.

**요청 헤더:**
```
Authorization: Bearer <access_token>
```

**요청 본문:**
```json
{
  "name": "NewHero",
  "class": "Rogue"
}
```

**응답 (201 Created):**
```json
{
  "success": true,
  "character": {
    "id": 3,
    "name": "NewHero",
    "class": "Rogue",
    "level": 1,
    "experience": 0,
    "hp": 74,
    "max_hp": 74,
    "mp": 28,
    "max_mp": 28,
    "stats": {
      "strength": 10,
      "dexterity": 15,
      "intelligence": 10,
      "wisdom": 8,
      "constitution": 12
    },
    "room_id": 1,
    "gold": 0
  }
}
```

### 인벤토리 (Inventory)

#### GET /api/v1/inventory/:character_id

캐릭터의 인벤토리를 조회한다.

**요청 헤더:**
```
Authorization: Bearer <access_token>
```

**응답 (200 OK):**
```json
{
  "success": true,
  "inventory": {
    "character_id": 1,
    "items": [
      {
        "item_id": 1,
        "name": "Iron Sword",
        "quantity": 1,
        "item_type": "Weapon"
      },
      {
        "item_id": 10,
        "name": "Health Potion",
        "quantity": 5,
        "item_type": "Consumable"
      }
    ],
    "gold": 1500,
    "capacity": 20
  }
}
```

### 경매 (Auction)

#### POST /api/v1/auction/listings

경매에 아이템을 등록한다.

**요청 헤더:**
```
Authorization: Bearer <access_token>
```

**요청 본문:**
```json
{
  "character_id": 1,
  "item_id": 1,
  "quantity": 1,
  "price": 500
}
```

**응답 (201 Created):**
```json
{
  "success": true,
  "listing": {
    "id": 100,
    "seller_id": 1,
    "item_id": 1,
    "item_name": "Iron Sword",
    "quantity": 1,
    "price": 500,
    "status": "active",
    "created_at": "2026-08-28T10:00:00Z",
    "expires_at": "2026-08-30T10:00:00Z"
  }
}
```

#### GET /api/v1/auction/search

경매 아이템을 검색한다.

**쿼리 파라미터:**
- `item_name`: 아이템 이름 (부분 일치)
- `min_price`: 최소 가격
- `max_price`: 최대 가격
- `item_type`: 아이템 타입
- `page`: 페이지 번호 (기본: 1)
- `per_page`: 페이지당 항목 수 (기본: 20)

**요청 헤더:**
```
Authorization: Bearer <access_token>
```

**예시:**
```
GET /api/v1/auction/search?item_name=sword&min_price=100&max_price=1000
```

**응답 (200 OK):**
```json
{
  "success": true,
  "listings": [
    {
      "id": 100,
      "seller_name": "Hero",
      "item_id": 1,
      "item_name": "Iron Sword",
      "quantity": 1,
      "price": 500,
      "created_at": "2026-08-28T10:00:00Z"
    }
  ],
  "pagination": {
    "page": 1,
    "per_page": 20,
    "total": 1,
    "total_pages": 1
  }
}
```

### 랭킹 (Ranking)

#### GET /api/v1/ranking

전체 랭킹을 조회한다.

**쿼리 파라미터:**
- `type`: 랭킹 타입 (level, combat)
- `limit`: 반환 항목 수 (기본: 10, 최대: 100)

**응답 (200 OK):**
```json
{
  "success": true,
  "ranking": {
    "type": "level",
    "entries": [
      {
        "rank": 1,
        "character_id": 5,
        "character_name": "Legend",
        "class": "Warrior",
        "level": 50,
        "score": 5025000
      },
      {
        "rank": 2,
        "character_id": 1,
        "character_name": "Hero",
        "class": "Mage",
        "level": 30,
        "score": 3010000
      }
    ],
    "updated_at": "2026-08-28T10:00:00Z"
  }
}
```

## 요청/응답 포맷 (JSON)

### 공통 응답 구조

```rust
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<Pagination>,
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct Pagination {
    pub page: u32,
    pub per_page: u32,
    pub total: u64,
    pub total_pages: u32,
}
```

## 에러 응답 포맷

| HTTP 상태 | 에러 코드 | 설명 |
|----------|----------|------|
| 400 | INVALID_REQUEST | 잘못된 요청 형식 |
| 401 | UNAUTHORIZED | 인증 필요 |
| 401 | INVALID_CREDENTIALS | 잘못된 인증 정보 |
| 401 | TOKEN_EXPIRED | 토큰 만료 |
| 403 | FORBIDDEN | 접근 권한 없음 |
| 404 | NOT_FOUND | 리소스 없음 |
| 409 | CONFLICT | 중복 리소스 |
| 422 | VALIDATION_ERROR | 입력 검증 실패 |
| 429 | RATE_LIMITED | 요청 빈도 초과 |
| 500 | INTERNAL_ERROR | 서버 내부 오류 |

```json
{
  "success": false,
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Character name must be 2-32 characters",
    "details": {
      "field": "name",
      "min_length": 2,
      "max_length": 32,
      "actual_length": 1
    }
  }
}
```

## 인증 (Bearer Token)

```rust
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    rejection::Reject,
};

pub struct AuthHeader(pub Claims);

#[derive(Debug)]
pub enum AuthError {
    MissingToken,
    InvalidToken(String),
    ExpiredToken,
}

impl Reject for AuthError {}

#[async_trait]
impl<S> FromRequestParts<S> for AuthHeader
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthError::MissingToken)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::MissingToken)?;

        let auth_manager = parts
            .extensions
            .get::<Arc<AuthManager>>()
            .ok_or(AuthError::MissingToken)?;

        let claims = auth_manager
            .validate_token(token)
            .map_err(|e| AuthError::InvalidToken(e.to_string()))?;

        Ok(AuthHeader(claims))
    }
}
```

## Rate Limiting

```rust
use tower::ServiceBuilder;
use tower_http::limit::RequestBodyLimitLayer;

let app = Router::new()
    .route("/api/v1/*path", any(handler))
    .layer(
        ServiceBuilder::new()
            .layer(RequestBodyLimitLayer::new(1024 * 1024))  // 1MB
            .layer(RateLimitLayer::new(
                100,  // 최대 100개 요청
                Duration::from_secs(60),  // 60초 윈도우
            ))
    );
```

## CORS 설정

```rust
use tower_http::cors::{CorsLayer, Any};

let cors = CorsLayer::new()
    .allow_origin(Any)
    .allow_methods([
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::DELETE,
    ])
    .allow_headers([
        header::CONTENT_TYPE,
        header::AUTHORIZATION,
    ])
    .max_age(Duration::from_secs(3600));

let app = Router::new()
    .route("/api/v1/*path", any(handler))
    .layer(cors);
```

## API 버전 관리

```
URL 기반 버전 관리:
  /api/v1/auth/login     ← 현재 버전
  /api/v2/auth/login     ← 차기 버전 (호환성 변경 시)

Accept 헤더 기반 (대안):
  Accept: application/vnd.the-protocol.v1+json
```

**버전 관리 정책:**
- 메이저 버전: 호환성 없는 변경 (`/api/v1` → `/api/v2`)
- 마이너 버전: 새로운 엔드포인트 추가 (동일 URL)
- 패치 버전: 버그 수정 (동일 URL)
