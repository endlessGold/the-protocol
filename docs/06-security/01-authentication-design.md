# 06-01 - 인증 시스템 설계 (미구현)

## 개요

The Protocol의 인증 시스템은 JWT 토큰 기반 인증을 핵심으로 하며, 세션 기반 인증과의 이중화를 지원한다. 현재 구현은 없으나, Hello 핸드셰이크에서 `auth_token` 필드가 이미 준비되어 있다.

## 인증 아키텍처

```
┌──────────────────────────────────────────────────────────┐
│                      클라이언트                           │
│  ┌──────────┐  ┌──────────────┐  ┌───────────────────┐ │
│  │ 로그인   │  │ 토큰 저장    │  │ 요청 시 토큰 전송  │ │
│  │ (credentials) │ (SecureStorage) │ (Authorization)│ │
│  └────┬─────┘  └──────────────┘  └────────┬──────────┘ │
└───────┼───────────────────────────────────┼─────────────┘
        │                                   │
        │  1. POST /auth/login              │
        │  2. Receive JWT + Refresh Token   │
        │  3. Bearer Token in Header        │
        │                                   │
┌───────▼───────────────────────────────────▼─────────────┐
│                    The Protocol Server                   │
│  ┌───────────────────────────────────────────────────┐  │
│  │                AuthMiddleware                     │  │
│  │  1. Extract Bearer Token                          │  │
│  │  2. Validate JWT (signature, expiry)              │  │
│  │  3. Attach Claims to Context                      │  │
│  └───────────────────────┬───────────────────────────┘  │
│                          │                               │
│  ┌───────────────────────▼───────────────────────────┐  │
│  │              Command Handlers                     │  │
│  │  Claims.permissions로 권한 검증                     │  │
│  └───────────────────────────────────────────────────┘  │
│                                                          │
│  ┌───────────────────────────────────────────────────┐  │
│  │              AuthManager                          │  │
│  │  - create_token()                                 │  │
│  │  - validate_token()                               │  │
│  │  - refresh_token()                                │  │
│  │  - revoke_token()                                 │  │
│  └───────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

## JWT 토큰 기반 인증

### 토큰 구조 (Claims)

```rust
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// 주체 식별자 (플레이어 ID)
    pub sub: u64,

    /// 토큰 만료 시간 (Unix timestamp)
    pub exp: usize,

    /// 토큰 발급 시간 (Unix timestamp)
    pub iat: usize,

    /// 플레이어 권한 목록
    pub permissions: Vec<String>,

    /// 세션 ID
    pub session_id: u64,

    /// 토큰 타입 (access / refresh)
    pub token_type: TokenType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TokenType {
    Access,
    Refresh,
}
```

**Claims 필드 상세:**

| 필드 | 타입 | 설명 | 예시 |
|------|------|------|------|
| sub | u64 | 플레이어 고유 ID | `12345` |
| exp | usize | 만료 Unix timestamp | `1724870400` |
| iat | usize | 발급 Unix timestamp | `1724866800` |
| permissions | Vec\<String\> | 권한 목록 | `["player.read", "combat.modify"]` |
| session_id | u64 | 세션 ID | `1` |
| token_type | TokenType | 토큰 타입 | `Access` |

### 토큰 생성/검증

```rust
use jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey};

pub struct AuthManager {
    secret: Vec<u8>,
    access_token_expiry: chrono::Duration,
    refresh_token_expiry: chrono::Duration,
}

impl AuthManager {
    pub fn new(secret: &str) -> Self {
        Self {
            secret: secret.as_bytes().to_vec(),
            access_token_expiry: chrono::Duration::hours(1),
            refresh_token_expiry: chrono::Duration::days(30),
        }
    }

    /// Access Token 생성
    pub fn create_access_token(
        &self,
        player_id: u64,
        permissions: Vec<String>,
        session_id: u64,
    ) -> Result<String, AuthError> {
        let now = Utc::now();
        let claims = Claims {
            sub: player_id,
            exp: (now + self.access_token_expiry).timestamp() as usize,
            iat: now.timestamp() as usize,
            permissions,
            session_id,
            token_type: TokenType::Access,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(&self.secret),
        )
        .map_err(|e| AuthError::TokenCreation(e.to_string()))
    }

    /// Refresh Token 생성
    pub fn create_refresh_token(
        &self,
        player_id: u64,
        session_id: u64,
    ) -> Result<String, AuthError> {
        let now = Utc::now();
        let claims = Claims {
            sub: player_id,
            exp: (now + self.refresh_token_expiry).timestamp() as usize,
            iat: now.timestamp() as usize,
            permissions: vec![],
            session_id,
            token_type: TokenType::Refresh,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(&self.secret),
        )
        .map_err(|e| AuthError::TokenCreation(e.to_string()))
    }

    /// 토큰 검증
    pub fn validate_token(&self, token: &str) -> Result<Claims, AuthError> {
        let validation = Validation::default();

        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(&self.secret),
            &validation,
        )
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
            jsonwebtoken::errors::ErrorKind::InvalidSignature => AuthError::InvalidSignature,
            _ => AuthError::TokenInvalid(e.to_string()),
        })?;

        Ok(token_data.claims)
    }

    /// Refresh Token으로 새 Access Token 발급
    pub fn refresh_access_token(
        &self,
        refresh_token: &str,
        permissions: Vec<String>,
    ) -> Result<(String, String), AuthError> {
        let claims = self.validate_token(refresh_token)?;

        if claims.token_type != TokenType::Refresh {
            return Err(AuthError::InvalidTokenType);
        }

        let new_access = self.create_access_token(
            claims.sub,
            permissions,
            claims.session_id,
        )?;

        let new_refresh = self.create_refresh_token(
            claims.sub,
            claims.session_id,
        )?;

        Ok((new_access, new_refresh))
    }
}
```

### 토큰 갱신 (Refresh Token)

```
Access Token 만료 시:
  1. 클라이언트가 Refresh Token으로 /api/v1/auth/refresh 호출
  2. 서버가 Refresh Token 검증
  3. 새 Access Token + Refresh Token 발급
  4. 기존 Refresh Token 폐기 (Rotation)

Refresh Token Rotation:
  - 매 갱신 시 새 Refresh Token 발급
  - 기존 토큰은 즉시 폐기
  - 탈취된 토큰의 재사용 방지
```

```rust
pub struct TokenRotation {
    revoked_tokens: Arc<DashSet<String>>,  // 폐기된 토큰 목록
    auth_manager: AuthManager,
}

impl TokenRotation {
    pub async fn rotate(
        &self,
        refresh_token: &str,
    ) -> Result<TokenPair, AuthError> {
        // 1. 기존 토큰 폐기
        self.revoke(refresh_token).await;

        // 2. 토큰 검증
        let claims = self.auth_manager.validate_token(refresh_token)?;

        // 3. 새 토큰 쌍 생성
        let new_access = self.auth_manager.create_access_token(
            claims.sub,
            vec![],
            claims.session_id,
        )?;
        let new_refresh = self.auth_manager.create_refresh_token(
            claims.sub,
            claims.session_id,
        )?;

        Ok(TokenPair {
            access_token: new_access,
            refresh_token: new_refresh,
        })
    }

    pub async fn revoke(&self, token: &str) {
        self.revoked_tokens.insert(token.to_string());
    }

    pub async fn is_revoked(&self, token: &str) -> bool {
        self.revoked_tokens.contains(token)
    }
}
```

## 패스워드 해싱 (Argon2)

```rust
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

pub struct PasswordManager;

impl PasswordManager {
    /// 패스워드 해싱
    pub fn hash_password(password: &str) -> Result<String, AuthError> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| AuthError::HashError(e.to_string()))?;

        Ok(hash.to_string())
    }

    /// 패스워드 검증
    pub fn verify_password(
        password: &str,
        stored_hash: &str,
    ) -> Result<bool, AuthError> {
        let parsed_hash = PasswordHash::new(stored_hash)
            .map_err(|e| AuthError::HashError(e.to_string()))?;

        let argon2 = Argon2::default();

        Ok(argon2
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    }
}
```

**Argon2 매개변수:**
| 매개변수 | 값 | 설명 |
|---------|-----|------|
| memory_cost | 19456 KB (19MB) | 메모리 사용량 |
| time_cost | 2 | 반복 횟수 |
| parallelism | 1 | 병렬 처리 스레드 수 |
| output_len | 32 | 해시 출력 길이 |

## OAuth2 지원 (미구현)

```rust
pub struct OAuth2Provider {
    pub provider: String,       // "google", "github", "discord"
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

// 미구현: 서드파티 OAuth2 제공자와의 통합
// Google, GitHub, Discord 등 OAuth2 제공자 지원
// OpenID Connect 기반 ID 토큰 검증
```

## 세션 기반 인증 (대안)

현재 `SessionManager`는 TCP 연결 기반 세션을 관리한다. JWT 없이도 세션 ID만으로 인증이 가능하다.

```rust
// 세션 기반 인증 (현재 구현 방식)
impl SessionManager {
    pub fn authenticate_session(
        &self,
        session_id: u64,
        player_id: u64,
        permissions: Vec<String>,
    ) -> Result<(), SessionError> {
        if let Some(mut session) = self.sessions.get_mut(&session_id) {
            session.set_player(player_id);
            session.state = SessionState::InGame;
            Ok(())
        } else {
            Err(SessionError::NotFound(session_id))
        }
    }
}

// Hello 핸드셰이크에서 인증
// Client → Server: Hello { auth_token: Some("token") }
// Server → Client: HelloAck { session_id, ... }
// 서버가 auth_token 검증 후 세션에 player_id 바인딩
```

**세션 기반 vs JWT 기반 비교:**

| 특성 | 세션 기반 | JWT 기반 |
|------|----------|---------|
| 상태 저장 | 서버 (메모리) | 클라이언트 (토큰) |
| 확장성 | 제한적 (세션 공유 필요) | 높음 (무상태) |
| 멀티 서버 | 세션 스토어 필요 | 불필요 |
| 보안 | 서버 측 제어 용이 | 탈취 시 위험 |
| 구현 복잡도 | 낮음 | 중간 |

## 로그인/로그아웃 플로우

### 로그인 플로우

```
┌──────────┐                              ┌──────────┐
│ 클라이언트 │                              │  서버    │
└────┬─────┘                              └────┬─────┘
     │                                         │
     │  1. Hello { auth_token: "token" }       │
     │────────────────────────────────────────>│
     │                                         │
     │           ┌─────────────────────┐       │
     │           │ 2. auth_token 검증   │       │
     │           │    - JWT 파싱        │       │
     │           │    - 서명 검증       │       │
     │           │    - 만료 확인       │       │
     │           │    - 권한 추출       │       │
     │           └──────────┬──────────┘       │
     │                      │                  │
     │                      ▼                  │
     │           ┌─────────────────────┐       │
     │           │ 3. 세션 생성         │       │
     │           │    player_id 바인딩  │       │
     │           │    상태: InGame      │       │
     │           └──────────┬──────────┘       │
     │                      │                  │
     │  4. HelloAck         │                  │
     │  { session_id, capabilities }           │
     │<────────────────────────────────────────│
     │                                         │
     │  5. Game Commands                       │
     │────────────────────────────────────────>│
```

### 로그아웃 플로우

```
┌──────────┐                              ┌──────────┐
│ 클라이언트 │                              │  서버    │
└────┬─────┘                              └────┬─────┘
     │                                         │
     │  1. Disconnect                          │
     │────────────────────────────────────────>│
     │                                         │
     │           ┌─────────────────────┐       │
     │           │ 2. 세션 정리         │       │
     │           │    - 세션 제거       │       │
     │           │    - 플레이어 상태 저장│      │
     │           │    - 타이머 정리     │       │
     │           └─────────────────────┘       │
     │                                         │
```

### 로그인 실패 플로우

```
┌──────────┐                              ┌──────────┐
│ 클라이언트 │                              │  서버    │
└────┬─────┘                              └────┬─────┘
     │                                         │
     │  1. Hello { auth_token: "invalid" }     │
     │────────────────────────────────────────>│
     │                                         │
     │           ┌─────────────────────┐       │
     │           │ 2. 토큰 검증 실패    │       │
     │           │    InvalidSignature  │       │
     │           └──────────┬──────────┘       │
     │                      │                  │
     │  3. Error            │                  │
     │  { message: "Invalid token" }           │
     │<────────────────────────────────────────│
     │                                         │
     │  4. Disconnect                          │
     │────────────────────────────────────────>│
```

## 다중 디바이스 지원

```rust
pub struct MultiDeviceManager {
    /// 플레이어별 활성 세션 목록
    active_sessions: DashMap<u64, Vec<ActiveSession>>,
    /// 최대 동시 접속 디바이스 수
    max_devices: usize,
}

pub struct ActiveSession {
    pub session_id: u64,
    pub device_type: String,     // "mobile", "desktop", "web"
    pub connected_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub ip_address: String,
}

impl MultiDeviceManager {
    pub fn can_connect(&self, player_id: u64) -> bool {
        self.active_sessions
            .get(&player_id)
            .map(|sessions| sessions.len() < self.max_devices)
            .unwrap_or(true)
    }

    pub fn register_session(
        &self,
        player_id: u64,
        session: ActiveSession,
    ) -> Result<(), AuthError> {
        if !self.can_connect(player_id) {
            return Err(AuthError::TooManyDevices);
        }

        self.active_sessions
            .entry(player_id)
            .or_insert_with(Vec::new)
            .push(session);

        Ok(())
    }

    pub fn disconnect_session(
        &self,
        player_id: u64,
        session_id: u64,
    ) {
        if let Some(mut sessions) = self.active_sessions.get_mut(&player_id) {
            sessions.retain(|s| s.session_id != session_id);
        }
    }

    pub fn get_active_sessions(&self, player_id: u64) -> Vec<ActiveSession> {
        self.active_sessions
            .get(&player_id)
            .map(|s| s.clone())
            .unwrap_or_default()
    }
}
```

**다중 디바이스 정책:**
- 기본 최대 동시 접속: 3개 디바이스
- 새 디바이스 접속 시 가장 오래된 세션 자동 종료 (선택적)
- 모든 세션에서 동일한 캐릭터 데이터 공유
- 세션 간 실시간 동기화 (WebSocket 사용 시)

## 에러 타입

```rust
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Token expired")]
    TokenExpired,

    #[error("Invalid token: {0}")]
    TokenInvalid(String),

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Invalid token type")]
    InvalidTokenType,

    #[error("Token creation failed: {0}")]
    TokenCreation(String),

    #[error("Password hash error: {0}")]
    HashError(String),

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Account locked")]
    AccountLocked,

    #[error("Too many devices")]
    TooManyDevices,

    #[error("Session not found: {0}")]
    SessionNotFound(u64),
}
```

## 보안 고려사항

- **토큰 탈취 대응**: Refresh Token Rotation으로 탈취 토큰 재사용 방지
- **무차별 대입 공격**: Rate Limiting + 계정 잠금 (5회 실패 시 15분 잠금)
- **시크릿 키 관리**: 환경 변수 또는 Vault에서 로드 (하드코딩 금지)
- **토큰 만료**: Access Token 1시간, Refresh Token 30일
- **폐기 목록**: 로그아웃 시 즉시 토큰 폐기
