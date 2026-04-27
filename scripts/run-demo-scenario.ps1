param(
    [Parameter(Mandatory = $true)]
    [ValidateSet(
        "duplicate_request_same_idempotency",
        "retryable_provider_outage",
        "terminal_provider_rejection",
        "timeout_unknown_outcome",
        "delayed_webhook_resolves_unknown",
        "duplicate_webhook_event",
        "callback_delivery_failure_and_retry",
        "reconciliation_mismatch",
        "worker_crash_and_recovery",
        "stale_pending_requires_recon"
    )]
    [string]$Scenario,

    [string]$ApiBaseUrl = "http://127.0.0.1:3000",
    [string]$MockProviderBaseUrl = "http://127.0.0.1:3010",
    [string]$DemoReceiverBaseUrl = "http://127.0.0.1:3020",
    [string]$ApiBearerToken = $env:API_BEARER_TOKEN,
    [string]$OutputRoot = "",
    [int]$TimeoutSeconds = 45
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ApiBearerToken)) {
    throw "API_BEARER_TOKEN or -ApiBearerToken is required."
}

$ScriptRoot = if (-not [string]::IsNullOrWhiteSpace($PSScriptRoot)) {
    $PSScriptRoot
} elseif ($MyInvocation.MyCommand.Path) {
    Split-Path -Parent $MyInvocation.MyCommand.Path
} else {
    (Get-Location).Path
}

$RepoRoot = (Resolve-Path (Join-Path $ScriptRoot "..")).Path

if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $OutputRoot = Join-Path $RepoRoot "demo-output"
}

$RunStamp = Get-Date -Format "yyyyMMdd-HHmmss"
$ScenarioDir = Join-Path $OutputRoot "$Scenario-$RunStamp"
New-Item -ItemType Directory -Force -Path $ScenarioDir | Out-Null

function Write-Artifact {
    param(
        [string]$Name,
        [Parameter(Mandatory = $true)]
        $Value
    )

    $Path = Join-Path $ScenarioDir $Name
    $Value | ConvertTo-Json -Depth 20 | Set-Content -Path $Path
    return $Path
}

function Get-ApiHeaders {
    param([string]$IdempotencyKey)

    $Headers = @{
        Authorization = "Bearer $ApiBearerToken"
    }

    if ($IdempotencyKey) {
        $Headers["Idempotency-Key"] = $IdempotencyKey
    }

    return $Headers
}

function Invoke-JsonRequest {
    param(
        [ValidateSet("GET", "POST", "DELETE")]
        [string]$Method,
        [string]$Url,
        [hashtable]$Headers,
        $Body = $null
    )

    try {
        if ($null -ne $Body) {
            return Invoke-RestMethod -Method $Method -Uri $Url -Headers $Headers -ContentType "application/json" -Body ($Body | ConvertTo-Json -Depth 20)
        }

        return Invoke-RestMethod -Method $Method -Uri $Url -Headers $Headers
    } catch {
        $Response = $_.Exception.Response
        if ($null -eq $Response) {
            throw
        }

        $Reader = New-Object System.IO.StreamReader($Response.GetResponseStream())
        $Payload = $Reader.ReadToEnd()
        throw "HTTP $($Response.StatusCode.value__) from $Method ${Url}: $Payload"
    }
}

function New-MerchantReference {
    param(
        [string]$Label,
        [string[]]$Directives = @()
    )

    $Base = "demo-$Label-$RunStamp"
    if ($Directives.Count -eq 0) {
        return $Base
    }

    return ($Base + "|" + ($Directives -join "|"))
}

function New-PaymentIntent {
    param(
        [string]$MerchantReference,
        [string]$IdempotencyKey,
        [string]$Provider = "mockpay",
        [string]$CallbackUrl = $null
    )

    $Body = @{
        merchant_reference = $MerchantReference
        amount_minor = 5000
        currency = "NGN"
        provider = $Provider
    }

    if (-not [string]::IsNullOrWhiteSpace($CallbackUrl)) {
        $Body["callback_url"] = $CallbackUrl
    }

    return Invoke-JsonRequest -Method POST -Url "$ApiBaseUrl/payment-intents" -Headers (Get-ApiHeaders -IdempotencyKey $IdempotencyKey) -Body $Body
}

function Get-Receipt {
    param([string]$IntentId)
    return Invoke-JsonRequest -Method GET -Url "$ApiBaseUrl/payment-intents/$IntentId/receipt" -Headers (Get-ApiHeaders)
}

function Wait-ForReceipt {
    param(
        [string]$IntentId,
        [scriptblock]$Condition,
        [string]$Description
    )

    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $Receipt = Get-Receipt -IntentId $IntentId
        if (& $Condition $Receipt) {
            return $Receipt
        }

        Start-Sleep -Seconds 1
    } while ((Get-Date) -lt $Deadline)

    throw "Timed out waiting for receipt condition: $Description"
}

function Reset-DemoReceiver {
    Invoke-JsonRequest -Method POST -Url "$DemoReceiverBaseUrl/admin/reset" -Headers @{}
}

function Get-DemoReceiverAttempts {
    param([string]$Key)
    return Invoke-JsonRequest -Method GET -Url "$DemoReceiverBaseUrl/callbacks/$Key" -Headers @{}
}

function Reset-MockProvider {
    Invoke-JsonRequest -Method POST -Url "$MockProviderBaseUrl/mock-provider/admin/reset" -Headers @{}
}

function Delete-MockProviderPayment {
    param([string]$ProviderReference)
    Invoke-JsonRequest -Method DELETE -Url "$MockProviderBaseUrl/mock-provider/payments/$ProviderReference" -Headers @{}
}

function Invoke-Reconciler {
    param([string]$IntentId)

    Push-Location $RepoRoot
    try {
        $env:RECONCILE_INTENT_IDS = $IntentId
        cargo run -p reconciler | Out-Host
    } finally {
        Remove-Item Env:RECONCILE_INTENT_IDS -ErrorAction SilentlyContinue
        Pop-Location
    }
}

function Start-WorkerProcess {
    param(
        [hashtable]$EnvironmentOverrides
    )

    $CommandLines = @()
    foreach ($Entry in $EnvironmentOverrides.GetEnumerator()) {
        $EscapedValue = $Entry.Value.ToString().Replace("'", "''")
        $CommandLines += "`$env:$($Entry.Key)='$EscapedValue'"
    }
    $CommandLines += "Set-Location '$RepoRoot'"
    $CommandLines += "cargo run -p worker"
    $InlineCommand = $CommandLines -join "; "

    return Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList @(
        "-NoProfile",
        "-Command",
        $InlineCommand
    )
}

function Run-DuplicateRequestScenario {
    $IdempotencyKey = "idem-$Scenario-$RunStamp"
    $MerchantReference = New-MerchantReference -Label "duplicate-request"

    $First = New-PaymentIntent -MerchantReference $MerchantReference -IdempotencyKey $IdempotencyKey
    $Second = New-PaymentIntent -MerchantReference $MerchantReference -IdempotencyKey $IdempotencyKey
    $Receipt = Get-Receipt -IntentId $First.intent_id

    if ($First.intent_id -ne $Second.intent_id) {
        throw "Expected duplicate request to return the same intent id."
    }

    Write-Artifact -Name "first-response.json" -Value $First | Out-Null
    Write-Artifact -Name "second-response.json" -Value $Second | Out-Null
    Write-Artifact -Name "receipt.json" -Value $Receipt | Out-Null

    return @{
        scenario = $Scenario
        intent_id = $First.intent_id
        note = "duplicate idempotent request returned the same lineage"
    }
}

function Run-RetryableProviderOutageScenario {
    $Response = New-PaymentIntent -MerchantReference (New-MerchantReference -Label "retryable-outage" -Directives @("#scenario=retryable_infra_error")) -IdempotencyKey "idem-$Scenario-$RunStamp"
    $Receipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "retry_scheduled state" -Condition {
        param($Receipt)
        $Receipt.summary.current_state -eq "retry_scheduled"
    }

    Write-Artifact -Name "receipt.json" -Value $Receipt | Out-Null
    return @{
        scenario = $Scenario
        intent_id = $Response.intent_id
        state = $Receipt.summary.current_state
        note = "retryable provider outage scheduled a safe retry"
    }
}

function Run-TerminalProviderRejectionScenario {
    $Response = New-PaymentIntent -MerchantReference (New-MerchantReference -Label "terminal-rejection" -Directives @("#scenario=terminal_failure")) -IdempotencyKey "idem-$Scenario-$RunStamp"
    $Receipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "failed_terminal state" -Condition {
        param($Receipt)
        $Receipt.summary.current_state -eq "failed_terminal"
    }

    Write-Artifact -Name "receipt.json" -Value $Receipt | Out-Null
    return @{
        scenario = $Scenario
        intent_id = $Response.intent_id
        state = $Receipt.summary.current_state
        note = "terminal provider rejection stopped without retry"
    }
}

function Run-TimeoutUnknownOutcomeScenario {
    $Response = New-PaymentIntent -MerchantReference (New-MerchantReference -Label "timeout-unknown" -Directives @("#scenario=timeout_after_acceptance", "#provider_webhook=off")) -IdempotencyKey "idem-$Scenario-$RunStamp"
    $Receipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "unknown_outcome state" -Condition {
        param($Receipt)
        $Receipt.summary.current_state -eq "unknown_outcome"
    }

    Write-Artifact -Name "receipt.json" -Value $Receipt | Out-Null
    return @{
        scenario = $Scenario
        intent_id = $Response.intent_id
        state = $Receipt.summary.current_state
        note = "timeout preserved ambiguity instead of retrying blindly"
    }
}

function Run-DelayedWebhookResolvesUnknownScenario {
    $Response = New-PaymentIntent -MerchantReference (New-MerchantReference -Label "timeout-webhook" -Directives @("#scenario=timeout_after_acceptance", "#resolution_delay_ms=3500")) -IdempotencyKey "idem-$Scenario-$RunStamp"
    $Receipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "succeeded state after webhook" -Condition {
        param($Receipt)
        $Receipt.summary.current_state -eq "succeeded" -and $Receipt.provider_webhooks.total_events -ge 1
    }

    Write-Artifact -Name "receipt.json" -Value $Receipt | Out-Null
    return @{
        scenario = $Scenario
        intent_id = $Response.intent_id
        state = $Receipt.summary.current_state
        note = "delayed provider webhook resolved the ambiguous timeout safely"
    }
}

function Run-DuplicateWebhookScenario {
    $Response = New-PaymentIntent -MerchantReference (New-MerchantReference -Label "duplicate-webhook" -Directives @("#scenario=duplicate_webhook", "#resolution_delay_ms=1200")) -IdempotencyKey "idem-$Scenario-$RunStamp"
    $Receipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "succeeded with deduplicated webhook" -Condition {
        param($Receipt)
        $Receipt.summary.current_state -eq "succeeded" -and $Receipt.provider_webhooks.total_events -ge 1
    }

    Write-Artifact -Name "receipt.json" -Value $Receipt | Out-Null
    return @{
        scenario = $Scenario
        intent_id = $Response.intent_id
        stored_webhook_events = $Receipt.provider_webhooks.total_events
        note = "duplicate provider webhook was harmless after deduplication"
    }
}

function Run-CallbackRetryScenario {
    Reset-DemoReceiver | Out-Null
    $CallbackKey = "callback-$RunStamp"
    $CallbackUrl = "$DemoReceiverBaseUrl/callbacks/fail_once/$CallbackKey"
    $Response = New-PaymentIntent -MerchantReference (New-MerchantReference -Label "callback-retry" -Directives @("#scenario=immediate_success")) -IdempotencyKey "idem-$Scenario-$RunStamp" -CallbackUrl $CallbackUrl
    $Receipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "callback eventually delivered after retry" -Condition {
        param($Receipt)
        $Receipt.callbacks.delivered_count -ge 1 -and $Receipt.callbacks.delivery_attempt_count -ge 2
    }
    $ReceiverAttempts = Get-DemoReceiverAttempts -Key $CallbackKey

    Write-Artifact -Name "receipt.json" -Value $Receipt | Out-Null
    Write-Artifact -Name "receiver-attempts.json" -Value $ReceiverAttempts | Out-Null
    return @{
        scenario = $Scenario
        intent_id = $Response.intent_id
        delivery_attempt_count = $Receipt.callbacks.delivery_attempt_count
        note = "callback delivery failed once, retried, and then succeeded without re-executing payment"
    }
}

function Run-ReconciliationMismatchScenario {
    Reset-MockProvider | Out-Null
    $Response = New-PaymentIntent -MerchantReference (New-MerchantReference -Label "recon-mismatch" -Directives @("#scenario=immediate_success", "#provider_webhook=off")) -IdempotencyKey "idem-$Scenario-$RunStamp"
    $SucceededReceipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "succeeded state before provider deletion" -Condition {
        param($Receipt)
        $Receipt.summary.current_state -eq "succeeded"
    }

    Delete-MockProviderPayment -ProviderReference $SucceededReceipt.summary.provider_reference | Out-Null
    Invoke-Reconciler -IntentId $Response.intent_id

    $Receipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "manual_review after mismatch reconciliation" -Condition {
        param($Receipt)
        $Receipt.summary.current_state -eq "manual_review"
    }

    Write-Artifact -Name "receipt.json" -Value $Receipt | Out-Null
    return @{
        scenario = $Scenario
        intent_id = $Response.intent_id
        state = $Receipt.summary.current_state
        note = "reconciliation surfaced provider-missing mismatch instead of silently trusting stale success"
    }
}

function Run-WorkerCrashRecoveryScenario {
    Write-Warning "This scenario assumes no long-running worker is currently active."

    $Response = New-PaymentIntent -MerchantReference (New-MerchantReference -Label "worker-crash" -Directives @("#scenario=immediate_success", "#provider_webhook=off")) -IdempotencyKey "idem-$Scenario-$RunStamp"
    $CrashWorker = Start-WorkerProcess -EnvironmentOverrides @{
        WORKER_ID = "worker-crash"
        WORKER_EXIT_AFTER_FIRST_LEASE = "true"
        LEASE_SECS = "4"
        POLL_INTERVAL_MS = "200"
    }

    try {
        $LeasedReceipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "leased by simulated crash worker" -Condition {
            param($Receipt)
            @($Receipt.timeline | Where-Object {
                $_.kind -eq "lease_acquired" -and $_.detail -match "worker=worker-crash"
            }).Count -ge 1
        }
        Write-Artifact -Name "leased-receipt.json" -Value $LeasedReceipt | Out-Null

        $CrashWorker.WaitForExit(15000) | Out-Null
        if (-not $CrashWorker.HasExited) {
            throw "Timed out waiting for the simulated crash worker to exit."
        }

        $RecoveryWorker = Start-WorkerProcess -EnvironmentOverrides @{
            WORKER_ID = "worker-recovery"
            LEASE_SECS = "4"
            POLL_INTERVAL_MS = "200"
        }

        try {
            $Receipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "succeeded after lease recovery" -Condition {
                param($Receipt)
                $LeaseAcquisitions = @($Receipt.timeline | Where-Object { $_.kind -eq "lease_acquired" }).Count
                $RecoveredByNewWorker = @($Receipt.timeline | Where-Object {
                    $_.kind -eq "lease_acquired" -and $_.detail -match "worker=worker-recovery"
                }).Count -ge 1
                $Receipt.summary.current_state -eq "succeeded" -and $LeaseAcquisitions -ge 2 -and $RecoveredByNewWorker
            }

            Write-Artifact -Name "receipt.json" -Value $Receipt | Out-Null
            return @{
                scenario = $Scenario
                intent_id = $Response.intent_id
                state = $Receipt.summary.current_state
                note = "stale lease was recovered after a simulated worker crash"
            }
        } finally {
            if (-not $RecoveryWorker.HasExited) {
                Stop-Process -Id $RecoveryWorker.Id -Force
            }
        }
    } finally {
        if (-not $CrashWorker.HasExited) {
            Stop-Process -Id $CrashWorker.Id -Force
        }
    }
}

function Run-StalePendingRequiresReconScenario {
    $Response = New-PaymentIntent -MerchantReference (New-MerchantReference -Label "stale-pending" -Directives @("#scenario=pending_then_resolves", "#provider_webhook=off", "#resolution_delay_ms=3000")) -IdempotencyKey "idem-$Scenario-$RunStamp"
    $PendingReceipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "provider_pending state" -Condition {
        param($Receipt)
        $Receipt.summary.current_state -eq "provider_pending"
    }

    Start-Sleep -Seconds 4
    Invoke-Reconciler -IntentId $Response.intent_id

    $Receipt = Wait-ForReceipt -IntentId $Response.intent_id -Description "reconciled success after stale pending" -Condition {
        param($Receipt)
        $Receipt.summary.current_state -eq "reconciled"
    }

    Write-Artifact -Name "pending-receipt.json" -Value $PendingReceipt | Out-Null
    Write-Artifact -Name "receipt.json" -Value $Receipt | Out-Null
    return @{
        scenario = $Scenario
        intent_id = $Response.intent_id
        state = $Receipt.summary.current_state
        note = "stale provider_pending intent was resolved later through reconciliation"
    }
}

$Result = switch ($Scenario) {
    "duplicate_request_same_idempotency" { Run-DuplicateRequestScenario }
    "retryable_provider_outage" { Run-RetryableProviderOutageScenario }
    "terminal_provider_rejection" { Run-TerminalProviderRejectionScenario }
    "timeout_unknown_outcome" { Run-TimeoutUnknownOutcomeScenario }
    "delayed_webhook_resolves_unknown" { Run-DelayedWebhookResolvesUnknownScenario }
    "duplicate_webhook_event" { Run-DuplicateWebhookScenario }
    "callback_delivery_failure_and_retry" { Run-CallbackRetryScenario }
    "reconciliation_mismatch" { Run-ReconciliationMismatchScenario }
    "worker_crash_and_recovery" { Run-WorkerCrashRecoveryScenario }
    "stale_pending_requires_recon" { Run-StalePendingRequiresReconScenario }
}

$Result["artifact_dir"] = $ScenarioDir
$Result | ConvertTo-Json -Depth 10
