{{/*
Expand the name of the chart.
*/}}
{{- define "mcp-proxy.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "mcp-proxy.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "mcp-proxy.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "mcp-proxy.labels" -}}
helm.sh/chart: {{ include "mcp-proxy.chart" . }}
{{ include "mcp-proxy.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "mcp-proxy.selectorLabels" -}}
app.kubernetes.io/name: {{ include "mcp-proxy.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Resolve the MCP auth token argument value.
Returns the token directly if set, otherwise a Kubernetes env var reference.
*/}}
{{- define "mcp-proxy.mcpAuthTokenArg" -}}
{{- if .Values.proxy.mcpServer.auth.token -}}
{{- .Values.proxy.mcpServer.auth.token | quote -}}
{{- else -}}
{{- printf "$(%s)" .Values.proxy.mcpServer.auth.envVar -}}
{{- end -}}
{{- end }}

{{/*
Resolve the shared secret argument value.
Returns the secret directly if set, otherwise a Kubernetes env var reference.
*/}}
{{- define "mcp-proxy.sharedSecretArg" -}}
{{- if .Values.proxy.auth.sharedSecret.value -}}
{{- .Values.proxy.auth.sharedSecret.value | quote -}}
{{- else -}}
{{- printf "$(%s)" .Values.proxy.auth.sharedSecret.envVar -}}
{{- end -}}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "mcp-proxy.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "mcp-proxy.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}
