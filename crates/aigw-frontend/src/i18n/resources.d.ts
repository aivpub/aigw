/**
 * TD-008b: typed translation resources.
 *
 * Generated from crates/aigw-frontend/src/i18n/locales/en.json. Augmenting
 * i18next CustomTypeOptions makes t('key') compile-time checked in files that
 * import useTranslation, without flipping the global strict flag.
 *
 * Regenerate after editing en.json with:
 *   scripts/fe-i18n-types
 */
import "i18next";

declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: {
      translation: {
  common: {
    save: string;
    cancel: string;
    delete: string;
    create: string;
    edit: string;
    search: string;
    loading: string;
    noResults: string;
    removeAll: string;
    confirm: string;
    close: string;
    back: string;
    yes: string;
    no: string;
    enabled: string;
    disabled: string;
    active: string;
    inactive: string;
    all: string;
    none: string;
    unknown: string;
    refresh: string;
    copy: string;
    copied: string;
    export: string;
    import: string;
    required: string;
    optional: string;
    status: string;
    actions: string;
    details: string;
    reset: string;
    filter: string;
    clear: string;
    submit: string;
    saving: string;
    deleting: string;
    creating: string;
    editing: string;
    select: string;
    optionalField: string;
    unlimited: string;
    settings: string;
    retry: string;
    min: string;
    max: string;
    alias: string;
    name: string;
    token: string;
    user: string;
    spend: string;
    deletedAt: string;
    deleted: string;
    searchModels: string;
    loadingModels: string;
    noMatchingModels: string;
    copyFailed: string;
  },
  sidebar: {
    brand: string;
    admin: string;
    groups: {
      aiGateway: string;
      observability: string;
      accessControl: string;
      settings: string;
    },
    nav: {
      keys: string;
      models: string;
      playground: string;
      usage: string;
      spendLogs: string;
      users: string;
      teams: string;
      organizations: string;
      routerSettings: string;
      jobs: string;
      budgets: string;
      proxies: string;
    },
    expandSidebar: string;
    collapseSidebar: string;
  },
  login: {
    title: string;
    description: string;
    usernamePlaceholder: string;
    passwordPlaceholder: string;
    signIn: string;
    signingIn: string;
    errorEmpty: string;
    errorAuth: string;
  },
  header: {
    expandSidebar: string;
    collapseSidebar: string;
    logout: string;
    switchLanguage: string;
  },
  usage: {
    title: string;
    description: string;
    datePresets: {
      today: string;
      "3d": string;
      "7d": string;
      "30d": string;
      custom: string;
    },
    cards: {
      spend: string;
      requests: string;
      ok: string;
      failed: string;
      tokens: string;
      rate: string;
    },
    trend: string;
    tabSpend: string;
    tabTokens: string;
    tabRequests: string;
    tabChart: string;
    noData: string;
    cache: string;
    chart: {
      output: string;
      cacheWrite: string;
      cacheRead: string;
      input: string;
      success: string;
      spend: string;
      tokens: string;
      requests: string;
      promptTokens: string;
      completionTokens: string;
      totalTokens: string;
      successCount: string;
      failedCount: string;
      totalRequests: string;
    },
    topKeys: string;
    spendByProvider: string;
    spendByModelGroup: string;
    spendByModel: string;
    others: string;
  },
  dashboard: {
    title: string;
    description: string;
    totalSpend: string;
    periodSpend: string;
    totalRequests: string;
    spendByModel: string;
    spendByProvider: string;
    spendLogs: string;
    from: string;
    to: string;
    noData: string;
    noSpendLogs: string;
    tokens: string;
    table: {
      time: string;
      model: string;
      tokens: string;
      cost: string;
      status: string;
    },
  },
  keys: {
    title: string;
    description: string;
    newKey: string;
    allKeys: string;
    searchPlaceholder: string;
    noKeys: string;
    noMatch: string;
    viewMode: {
      active: string;
      deleted: string;
    },
    table: {
      alias: string;
      name: string;
      token: string;
      user: string;
      models: string;
      spend: string;
      budget: string;
      resetPeriod: string;
      expires: string;
      created: string;
      status: string;
      actions: string;
      deletedAt: string;
    },
    allModels: string;
    blocked: string;
    active: string;
    userTooltip: {
      userId: string;
      alias: string;
      email: string;
    },
    mobile: {
      spent: string;
      expires: string;
      created: string;
    },
    createDialog: {
      title: string;
      description: string;
      nameLabel: string;
      namePlaceholder: string;
      aliasLabel: string;
      aliasPlaceholder: string;
      modelsLabel: string;
      budgetLabel: string;
      budgetDurationLabel: string;
      budgetDurationOptions: {
        none: string;
        daily: string;
        weekly: string;
        monthly: string;
      },
      softBudgetLabel: string;
      tpmLabel: string;
      rpmLabel: string;
      expiresLabel: string;
      generateBtn: string;
      created: string;
      savedBtn: string;
      userSelectorLabel: string;
      userSelectorNone: string;
    },
    editDialog: {
      title: string;
      description: string;
      nameLabel: string;
      namePlaceholder: string;
      aliasLabel: string;
      modelsLabel: string;
      budgetLabel: string;
      tpmLabel: string;
      rpmLabel: string;
      expiresLabel: string;
      saveBtn: string;
    },
    deleteDialog: {
      title: string;
      description: string;
      confirm: string;
    },
    toast: {
      created: string;
      updated: string;
      deleted: string;
      unblocked: string;
      blocked: string;
      copied: string;
      copyFailed: string;
    },
    noDeletedRecords: string;
    deletedKeys: string;
    tpmLabel: string;
    rpmLabel: string;
  },
  models: {
    title: string;
    description: string;
    searchPlaceholder: string;
    tabModelGroups: string;
    tabCredentials: string;
    tabHealth: string;
    allModels: string;
    deletedModels: string;
    newModel: string;
    noModels: string;
    noMatch: string;
    noDeletedRecords: string;
    empty: string;
    costInput: string;
    costOutput: string;
    costCacheRead: string;
    costCacheWrite: string;
    detailId: string;
    detailCreatedBy: string;
    detailUpdatedBy: string;
    mobileProvider: string;
    mobileUpstream: string;
    mobileCost: string;
    mobileCreated: string;
    table: {
      modelName: string;
      modelId: string;
      provider: string;
      upstreamModel: string;
      status: string;
      cost: string;
      created: string;
      actions: string;
      deletedAt: string;
    },
    dialog: {
      title: {
        create: string;
        edit: string;
      },
      createDescription: string;
      editDescription: string;
      modelNameRequired: string;
      modelName: {
        label: string;
        description: string;
        placeholder: string;
      },
      upstreamModel: {
        label: string;
        description: string;
      },
      provider: {
        label: string;
        description: string;
      },
      authentication: string;
      apiKey: {
        tab: string;
        label: string;
        placeholder: string;
      },
      apiBase: {
        label: string;
        placeholder: string;
      },
      credential: {
        tab: string;
        label: string;
        description: string;
        new: string;
        selectPlaceholder: string;
      },
      pricingSection: string;
      inputPrice: {
        label: string;
        description: string;
      },
      outputPrice: {
        label: string;
        description: string;
      },
      cacheReadPrice: {
        label: string;
        description: string;
      },
      cacheWritePrice: {
        label: string;
        description: string;
      },
      rateLimits: string;
      rpm: {
        label: string;
        description: string;
      },
      tpm: {
        label: string;
        description: string;
      },
      chatTemplateSection: string;
      systemMessage: {
        label: string;
        description: string;
      },
      selectProvider: string;
      selectCredential: string;
      autoDetect: string;
      strictOption: string;
      looseOption: string;
      saveChanges: string;
      advancedSection: string;
      advancedHint: string;
      advancedPlaceholder: string;
    },
    deleteDialog: {
      title: string;
      description: string;
    },
    credentials: {
      new: string;
      allCredentials: string;
      empty: string;
      newCredential: string;
      editCredential: string;
      deleteCredential: string;
      name: string;
      provider: string;
      apiBase: string;
      apiKey: string;
      actions: string;
      encryptDesc: string;
      deleteDescription: string;
      form: {
        nameLabel: string;
        namePlaceholder: string;
        providerLabel: string;
        providerPlaceholder: string;
        apiBaseLabel: string;
        apiBasePlaceholder: string;
        apiKeyLabel: string;
        apiKeyPlaceholder: string;
        credInfoLabel: string;
        credInfoPlaceholder: string;
        valuesJsonLabel: string;
        infoJsonLabel: string;
        advancedToggle: string;
        advancedJsonInvalid: string;
        credInfoJsonInvalid: string;
      },
      toast: {
        created: string;
        updated: string;
        saved: string;
        deleted: string;
        saveFailed: string;
        deleteFailed: string;
      },
      deleteDialog: {
        title: string;
      },
      description: string;
    },
    health: {
      title: string;
      description: string;
      checkBtn: string;
      checkAll: string;
      checking: string;
      reCheck: string;
      lastRun: string;
      lastOk: string;
      noRuns: string;
      noRunsHint: string;
      noErrors: string;
      checkingStatus: string;
      unknownError: string;
      healthCheckFailed: string;
      singleHealthCheckFailed: string;
      status: string;
      model: string;
      latency: string;
      lastSuccess: string;
      error: string;
      action: string;
    },
  },
  users: {
    title: string;
    description: string;
    newUser: string;
    allUsers: string;
    deletedUsers: string;
    deletedCardTitle: string;
    allCardTitle: string;
    searchPlaceholder: string;
    noUsers: string;
    noMatch: string;
    noDeletedRecords: string;
    alias: string;
    mobile: {
      id: string;
      deleted: string;
      spent: string;
      budget: string;
      noBudget: string;
      created: string;
    },
    table: {
      alias: string;
      userId: string;
      email: string;
      role: string;
      spend: string;
      resetPeriod: string;
      deletedAt: string;
      actions: string;
      keys: string;
    },
    viewMode: {
      active: string;
      deleted: string;
    },
    dialog: {
      titleCreate: string;
      titleEdit: string;
      descriptionCreate: string;
      descriptionEdit: string;
      emailLabel: string;
      emailPlaceholder: string;
      passwordLabel: string;
      passwordPlaceholder: string;
      aliasLabel: string;
      aliasPlaceholder: string;
      roleLabel: string;
      spendLabel: string;
      budgetLabel: string;
      budgetDurationLabel: string;
      softBudgetLabel: string;
      resetPasswordLabel: string;
      resetPasswordPlaceholder: string;
      tpmLabel: string;
      rpmLabel: string;
    },
    deleteDialog: {
      title: string;
      confirm: string;
    },
    toast: {
      created: string;
      updated: string;
      deleted: string;
      blocked: string;
      unblocked: string;
    },
  },
  orgs: {
    title: string;
    description: string;
    newOrg: string;
    allOrgs: string;
    deletedOrgs: string;
    deletedCardTitle: string;
    allCardTitle: string;
    searchPlaceholder: string;
    noOrgs: string;
    noMatch: string;
    noDeletedRecords: string;
    mobile: {
      id: string;
      deleted: string;
      budget: string;
      created: string;
    },
    table: {
      name: string;
      orgId: string;
      budgetId: string;
      spend: string;
      created: string;
      actions: string;
      deletedAt: string;
    },
    dialog: {
      titleCreate: string;
      titleEdit: string;
      titleDelete: string;
      nameLabel: string;
      namePlaceholder: string;
      confirmDelete: string;
    },
    toast: {
      created: string;
      updated: string;
      deleted: string;
    },
  },
  teams: {
    title: string;
    description: string;
    newTeam: string;
    allTeams: string;
    deletedTeams: string;
    deletedCardTitle: string;
    allCardTitle: string;
    searchPlaceholder: string;
    noTeams: string;
    noMatch: string;
    noDeletedRecords: string;
    membersCount: string;
    noBudget: string;
    active: string;
    blocked: string;
    mobile: {
      id: string;
      org: string;
      deleted: string;
      spent: string;
      budget: string;
      created: string;
    },
    table: {
      name: string;
      teamId: string;
      org: string;
      members: string;
      status: string;
      spend: string;
      budget: string;
      resetPeriod: string;
      created: string;
      actions: string;
      deletedAt: string;
    },
    dialog: {
      titleCreate: string;
      titleEdit: string;
      titleDelete: string;
      descriptionCreate: string;
      nameLabel: string;
      namePlaceholder: string;
      orgIdLabel: string;
      orgIdPlaceholder: string;
      budgetLabel: string;
      budgetDurationLabel: string;
      softBudgetLabel: string;
      budgetPlaceholder: string;
      confirmDelete: string;
    },
    toast: {
      created: string;
      updated: string;
      deleted: string;
    },
  },
  spendLogs: {
    title: string;
    description: string;
    timePresets: {
      "15m": string;
      "1h": string;
      "6h": string;
      "24h": string;
      "7d": string;
    },
    filters: {
      callRequestIdPlaceholder: string;
      modelPlaceholder: string;
      statusPlaceholder: string;
      status: string;
      all: string;
      success: string;
      failure: string;
      streaming: string;
      model: string;
      session: string;
      tokenRange: string;
      minTokPlaceholder: string;
      maxTokPlaceholder: string;
    },
    table: {
      callId: string;
      requestId: string;
      upstreamId: string;
      time: string;
      type: string;
      model: string;
      key: string;
      endUser: string;
      ip: string;
      tokens: string;
      cost: string;
      spend: string;
      duration: string;
      ttft: string;
      status: string;
      provider: string;
    },
    drawer: {
      title: string;
      callId: string;
      requestId: string;
      upstreamId: string;
      toolsCount: string;
      tabs: {
        prompt: string;
        response: string;
        raw: string;
        timing: string;
      },
      noData: string;
      noUpstreamId: string;
      tabVisual: string;
      tabRaw: string;
      noPromptData: string;
      noResponseData: string;
      loadError: string;
      meta: {
        start: string;
        end: string;
        group: string;
        provider: string;
        id: string;
        base: string;
        user: string;
        endUser: string;
        session: string;
        cache: string;
        cacheRead: string;
        cacheWrite: string;
        cacheHit: string;
        cacheKey: string;
        team: string;
        org: string;
        mcpTool: string;
      },
      imageTokens: string;
      imageTokensUpstream: string;
      imageTokensEstimated: string;
      timing: {
        total: string;
        upstream: string;
        queue: string;
        gatewayOverhead: string;
        notAvailable: string;
      },
      metadata: {
        model: string;
        provider: string;
        tokens: string;
        prompt: string;
        completion: string;
        cost: string;
        duration: string;
      },
      fetchError: string;
      tabDescription: string;
      tabParams: string;
    },
    noData: string;
    search: {
      placeholder: string;
    },
    status: {
      streaming: string;
    },
    liveTail: string;
    live: string;
    cardTitle: string;
    csv: {
      promptTokens: string;
      completionTokens: string;
      totalTokens: string;
      ttft: string;
      duration: string;
    },
  },
  playground: {
    title: string;
    description: string;
    systemMessage: string;
    userMessage: string;
    send: string;
    clearSession: string;
    getCode: string;
    selectModel: string;
    selectModelPlaceholder: string;
    placeholder: string;
    temperature: string;
    maxTokens: string;
    topP: string;
    freqPenalty: string;
    presPenalty: string;
    streaming: string;
    virtualKey: string;
    virtualKeySession: string;
    virtualKeyCustom: string;
    endpointType: string;
    endpointChat: string;
    endpointMessages: string;
    messagesHint: string;
    newChat: string;
    startConversation: string;
    startHint: string;
    modelLabel: string;
    streamLabel: string;
    selectModelToBegin: string;
    saveResend: string;
    error: string;
    requestFailed: string;
    noResponseBody: string;
    tokensSuffix: string;
    in: string;
    out: string;
    total: string;
    streamingStatus: string;
    response: string;
    attachImage: string;
    removeImage: string;
    imagePreview: string;
    imageTooLarge: string;
    expandSettings: string;
    collapseSettings: string;
    codeDialog: {
      title: string;
      curl: string;
      sdk: string;
    },
    heicUnsupported: string;
  },
  routerSettings: {
    title: string;
    description: string;
    global: string;
    key: string;
    team: string;
    reliability: string;
    reliabilityDesc: string;
    strategyDesc: string;
    selectKey: string;
    selectKeyLabel: string;
    selectKeyPlaceholder: string;
    selectKeyHint: string;
    selectTeam: string;
    selectTeamLabel: string;
    selectTeamPlaceholder: string;
    selectTeamHint: string;
    fields: {
      strategy: string;
      numRetries: string;
      numRetriesDesc: string;
      retryAfter: string;
      allowedFails: string;
      allowedFailsDesc: string;
      cooldown: string;
      cooldownDesc: string;
      ttl: string;
      ttlDesc: string;
    },
    saveGlobal: string;
    saveKey: string;
    saveTeam: string;
    toast: {
      globalSaved: string;
      keySaved: string;
      teamSaved: string;
      saveFailed: string;
    },
  },
  jobs: {
    title: string;
    description: string;
    trigger: {
      cron: string;
      manual: string;
    },
    noJobs: string;
    subTabs: {
      bodyArchive: string;
      budgetReset: string;
    },
    overview: string;
    archiveOverview: string;
    allJobs: string;
    autoArchive: string;
    storage: string;
    archivedRows: string;
    pendingRows: string;
    on: string;
    off: string;
    configured: string;
    notConfigured: string;
    noJobsYet: string;
    total: string;
    noJobsRegistered: string;
    storageNotConfigured: string;
    triggerArchive: string;
    triggerArchiveDesc: string;
    budgetReset: {
      overview: string;
      readyToReset: string;
      lastReset: string;
      nextCheck: string;
      triggerReset: string;
      triggerDesc: string;
      entityType: string;
      entityTypes: {
        all: string;
        keys: string;
        users: string;
        teams: string;
        orgs: string;
      },
      recentResets: string;
      entitiesReset: string;
      nothingToReset: string;
      neverReset: string;
      noRecentResets: string;
      nextTick: string;
      previewTitle: string;
      previewEmpty: string;
      willReset: string;
      confirmReset: string;
      resetSchedule: string;
      totalCount: string;
      readyOf: string;
    },
    startDate: string;
    endDate: string;
    batchSize: string;
    triggerJob: string;
    triggerToast: string;
    summary: string;
    steps: {
      stepKey: string;
      status: string;
      started: string;
      completed: string;
      retryCount: string;
      payload: string;
      result: string;
      error: string;
      rowsArchived: string;
      duration: string;
      expandPayload: string;
    },
    progress: string;
    noLogs: string;
    level: string;
    message: string;
    stepJobs: string;
    failedSteps: string;
    table: {
      jobId: string;
      type: string;
      status: string;
      steps: string;
      created: string;
      id: string;
      stepType: string;
      trigger: string;
      progress: string;
      ended: string;
    },
    status: {
      pending: string;
      running: string;
      completed: string;
      failed: string;
      partiallyFailed: string;
    },
    detail: {
      title: string;
      stepsTitle: string;
      logsTitle: string;
      overview: string;
      jobId: string;
      type: string;
      status: string;
      created: string;
      updated: string;
      stepKey: string;
      payload: string;
      result: string;
      duration: string;
      expandPayload: string;
    },
    logs: {
      stepKey: string;
      message: string;
      timestamp: string;
    },
    triggerDialog: {
      title: string;
      description: string;
      selectType: string;
      triggerBtn: string;
      triggered: string;
      closeBtn: string;
      toast: {
        triggered: string;
      },
    },
    disabled: string;
    noop: string;
    noDetailSteps: string;
    pagination: {
      showing: string;
      page: string;
    },
    toast: {
      loadStatsFailed: string;
      loadJobsFailed: string;
      loadDetailFailed: string;
      triggerFailed: string;
    },
  },
  health: {
    title: string;
    description: string;
    apiStatus: string;
    version: string;
    database: string;
    dbDescription: string;
    uptime: string;
    build: string;
    connections: string;
    idle: string;
    used: string;
    max: string;
    healthy: string;
    unhealthy: string;
    loading: string;
    min: string;
  },
  logViewer: {
    input: string;
    output: string;
    system: string;
    user: string;
    assistant: string;
    tool: string;
    toolCalls: string;
    function: string;
    arguments: string;
    result: string;
    response: string;
    text: string;
    usage: string;
    finishReason: string;
    prompt: string;
    raw: string;
    noContent: string;
    copy: string;
    parseError: string;
    error: string;
    finish: string;
    embeddingsNoVectors: string;
    embeddingsDims: string;
  },
  pagination: {
    showing: string;
    pageInfo: string;
    rows: string;
    perPage: string;
  },
  auth: {
    loginFailed: string;
  },
  budgets: {
    title: string;
    description: string;
    newBudget: string;
    allBudgets: string;
    searchPlaceholder: string;
    noBudgets: string;
    noMatch: string;
    table: {
      name: string;
      budgetId: string;
      limit: string;
      resetPeriod: string;
      softAlert: string;
      created: string;
      actions: string;
    },
    mobile: {
      noCycle: string;
      alert: string;
      created: string;
    },
    dialog: {
      titleCreate: string;
      titleEdit: string;
      titleDelete: string;
      descriptionCreate: string;
      descriptionEdit: string;
      descriptionDelete: string;
      nameLabel: string;
      namePlaceholder: string;
      maxBudgetLabel: string;
      maxBudgetPlaceholder: string;
      resetCycleLabel: string;
      softBudgetLabel: string;
      softBudgetPlaceholder: string;
      saveBtn: string;
    },
    resetCycleOptions: {
      none: string;
      daily: string;
      weekly: string;
      monthly: string;
    },
    toast: {
      created: string;
      updated: string;
      deleted: string;
    },
  },
  proxies: {
    title: string;
    description: string;
    newProxy: string;
    allProxies: string;
    searchPlaceholder: string;
    noProxies: string;
    noMatch: string;
    table: {
      name: string;
      proxyUrl: string;
      exitIp: string;
      country: string;
      latency: string;
      score: string;
      grade: string;
      status: string;
      expiresAt: string;
      actions: string;
    },
    grade: {
      A: string;
      B: string;
      C: string;
      D: string;
      F: string;
    },
    status: {
      active: string;
      inactive: string;
      expired: string;
    },
    dialog: {
      titleCreate: string;
      titleEdit: string;
      nameLabel: string;
      namePlaceholder: string;
      proxyUrlLabel: string;
      proxyUrlPlaceholder: string;
      proxyUrlHint: string;
      expiresAtLabel: string;
      saveBtn: string;
      nameRequired: string;
      urlRequired: string;
    },
    deleteDialog: {
      title: string;
      description: string;
    },
    quality: {
      title: string;
      score: string;
      grade: string;
      overall: string;
      overallHealthy: string;
      overallWarn: string;
      overallFailed: string;
      overallChallenge: string;
      target: string;
      status: string;
      latency: string;
      cfRay: string;
      message: string;
      lastCheckAt: string;
      noItems: string;
      itemStatus: {
        pass: string;
        warn: string;
        challenge: string;
        fail: string;
      },
    },
    toast: {
      created: string;
      updated: string;
      deleted: string;
      testDone: string;
      testFailed: string;
      qualityDone: string;
      qualityFailed: string;
      toggleDone: string;
      batchDone: string;
      inUseSkipped: string;
    },
  },
      };
    };
  }
}
