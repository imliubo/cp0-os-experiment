use std::collections::BTreeSet;

use cp0_manifest::Permission;
use serde::{Deserialize, Serialize};

pub const RISK_POLICY_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskTier {
    Standard,
    Elevated,
    High,
}

impl RiskTier {
    pub const ALL: [Self; 3] = [Self::Standard, Self::Elevated, Self::High];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Elevated => "elevated",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskReason {
    CameraCapture,
    HardwareControl,
    MicrophoneCapture,
    MultipleSensitiveCapabilities,
    NetworkAccess,
    RadioTransmit,
    UserDocuments,
}

impl RiskReason {
    pub const ALL: [Self; 7] = [
        Self::CameraCapture,
        Self::HardwareControl,
        Self::MicrophoneCapture,
        Self::MultipleSensitiveCapabilities,
        Self::NetworkAccess,
        Self::RadioTransmit,
        Self::UserDocuments,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CameraCapture => "camera-capture",
            Self::HardwareControl => "hardware-control",
            Self::MicrophoneCapture => "microphone-capture",
            Self::MultipleSensitiveCapabilities => "multiple-sensitive-capabilities",
            Self::NetworkAccess => "network-access",
            Self::RadioTransmit => "radio-transmit",
            Self::UserDocuments => "user-documents",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskAssessment {
    pub policy_version: u16,
    pub tier: RiskTier,
    pub reasons: Vec<RiskReason>,
}

pub fn classify_permissions(permissions: &[Permission]) -> RiskAssessment {
    let declared = permissions.iter().copied().collect::<BTreeSet<_>>();
    let mut reasons = BTreeSet::new();
    let mut sensitive_count = 0;
    let mut high = false;

    for permission in declared {
        let reason = match permission {
            Permission::NetworkClient => Some(RiskReason::NetworkAccess),
            Permission::DocumentsOpen => Some(RiskReason::UserDocuments),
            Permission::AudioCapture => Some(RiskReason::MicrophoneCapture),
            Permission::CameraCapture => Some(RiskReason::CameraCapture),
            Permission::RadioLora => {
                high = true;
                Some(RiskReason::RadioTransmit)
            }
            Permission::HardwareGpio => {
                high = true;
                Some(RiskReason::HardwareControl)
            }
            Permission::AudioPlayback | Permission::NotificationsPost => None,
        };
        if let Some(reason) = reason {
            sensitive_count += 1;
            reasons.insert(reason);
        }
    }
    if sensitive_count >= 2 {
        high = true;
        reasons.insert(RiskReason::MultipleSensitiveCapabilities);
    }
    let tier = if high {
        RiskTier::High
    } else if sensitive_count == 1 {
        RiskTier::Elevated
    } else {
        RiskTier::Standard
    };
    RiskAssessment {
        policy_version: RISK_POLICY_VERSION,
        tier,
        reasons: reasons.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_standard_single_sensitive_and_hardware_permissions() {
        assert_eq!(classify_permissions(&[]).tier, RiskTier::Standard);
        assert_eq!(
            classify_permissions(&[Permission::AudioPlayback, Permission::NotificationsPost]).tier,
            RiskTier::Standard
        );
        assert_eq!(
            classify_permissions(&[Permission::CameraCapture]),
            RiskAssessment {
                policy_version: RISK_POLICY_VERSION,
                tier: RiskTier::Elevated,
                reasons: vec![RiskReason::CameraCapture],
            }
        );
        assert_eq!(
            classify_permissions(&[Permission::HardwareGpio]).tier,
            RiskTier::High
        );
        assert_eq!(
            classify_permissions(&[Permission::RadioLora]).reasons,
            vec![RiskReason::RadioTransmit]
        );
    }

    #[test]
    fn promotes_multiple_sensitive_capabilities_and_deduplicates_input() {
        let assessment = classify_permissions(&[
            Permission::CameraCapture,
            Permission::NetworkClient,
            Permission::CameraCapture,
        ]);
        assert_eq!(assessment.tier, RiskTier::High);
        assert_eq!(
            assessment.reasons,
            vec![
                RiskReason::CameraCapture,
                RiskReason::MultipleSensitiveCapabilities,
                RiskReason::NetworkAccess,
            ]
        );
    }

    #[test]
    fn openapi_vocabulary_matches_the_rust_policy() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../schemas/store-control-v1.openapi.json"
        ))
        .unwrap();
        let tiers = schema
            .pointer("/components/schemas/RiskAssessment/properties/tier/enum")
            .and_then(serde_json::Value::as_array)
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(tiers, RiskTier::ALL.map(RiskTier::as_str).as_slice());
        let reasons = schema
            .pointer("/components/schemas/RiskAssessment/properties/reasons/items/enum")
            .and_then(serde_json::Value::as_array)
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(reasons, RiskReason::ALL.map(RiskReason::as_str).as_slice());
    }
}
