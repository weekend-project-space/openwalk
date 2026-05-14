pub const LIB: &str = r#"
;; Minimal helpers shared by Scheme tools.
;;
;; Usage:
;;   (def answer 42)
;;   (def (add a b) (+ a b))
;;   (defun add (a b) (+ a b))
;;   (open "https://example.com")
;;   (open "https://example.com" #t)
;;   (script-dir)
;;   (read-file-text "/tmp/example.js")
;;   (read-sibling-file "main.js")
;;   (js-run "main.js" args)
;;   (parse-args args)
;;   (call-with-input-file path proc)
;;   (call-with-output-file path proc)


(define-syntax def
  (syntax-rules ()
    ((_ name value)
     (define name value))
    ((_ (name . args) body ...)
     (define (name . args) body ...))))

(define-syntax defun
  (syntax-rules ()
    ((_ name formals body ...)
     (define (name . formals) body ...))))

(defun %browser-session-exists? (session-name)
  (let ((sessions (browser-list)))
    (let loop ((index 0))
      (if (>= index (vector-length sessions))
          #f
          (if (equal? session-name (vector-ref sessions index))
              #t
              (loop (+ index 1)))))))

(defun %tab-field (tab key)
  (let ((entry (assoc key tab)))
    (if entry (cdr entry) #f)))

(defun %tab-url-id-alist ()
  (let loop ((tabs (tab-list)) (pairs '()))
    (if (null? tabs)
        (reverse pairs)
        (let* ((tab (car tabs))
               (url (%tab-field tab "url"))
               (id (%tab-field tab "id")))
          (if (and url id)
              (loop (cdr tabs) (cons (cons url id) pairs))
              (loop (cdr tabs) pairs))))))

(defun %url-with-trailing-slash (url)
  (if (and (> (string-length url) 0)
           (not (char=? (string-ref url (- (string-length url) 1)) #\/)))
      (string-append url "/")
      url))

(defun %url-without-trailing-slash (url)
  (if (and (> (string-length url) 0)
           (char=? (string-ref url (- (string-length url) 1)) #\/))
      (substring url 0 (- (string-length url) 1))
      url))

(defun string-contains? (str sub)
  (let ((len (string-length str))
        (sub-len (string-length sub)))

    (let loop ((i 0))
      (cond
        ((> (+ i sub-len) len)
         #f)

        ((string=? (substring str i (+ i sub-len)) sub)
         #t)

        (else
         (loop (+ i 1)))))))

(defun %find-tab-by-url (url)
  (let ((pairs (%tab-url-id-alist)))
    (let loop ((rest pairs))
      (cond
        ((null? rest) #f)

        ((string-contains?
           (caar rest)
           url)
         (cdar rest))

        (else
         (loop (cdr rest)))))))

(defun %open-impl (url no-reuse-tab)
  (let ((session-name openwalk-session-name))
    (if (and session-name (%browser-session-exists? session-name))
        (if no-reuse-tab
            (tab-new url)
            (let ((tab-id (%find-tab-by-url url)))
              (if tab-id
                  (tab-select tab-id)
                  (tab-new url))))
        (browser-open url))))

(define-syntax open
  (syntax-rules ()
    ((_ url)
     (%open-impl url #f))
    ((_ url no-reuse-tab)
     (%open-impl url no-reuse-tab))))

(defun %string-last-index (text target)
  (let loop ((index (- (string-length text) 1)))
    (if (< index 0)
        #f
        (if (char=? (string-ref text index) target)
            index
            (loop (- index 1))))))

(defun %path-dirname (path)
  (let ((slash-index (%string-last-index path #\/)))
    (if slash-index
        (substring path 0 slash-index)
        ".")))

(defun script-dir ()
  (%path-dirname openwalk-script-path))

(defun %read-port-all-text (port)
  (let ((out (open-output-string)))
    (do ((chunk (read-string 4096 port)
                (read-string 4096 port)))
        ((eof-object? chunk)
         (get-output-string out))
      (write-string chunk out))))

(defun read-file-text (path)
  (let ((port (open-input-file path)))
    (let ((text (%read-port-all-text port)))
      (close-input-port port)
      text)))

(defun read-sibling-file (name)
  (read-file-text
    (string-append (script-dir) "/" name)))

(defun %parse-args-error (message)
  (error (string-append "parse-args: " message)))

(defun %string-prefix? (prefix text)
  (let ((prefix-length (string-length prefix))
        (text-length (string-length text)))
    (if (> prefix-length text-length)
        #f
        (let loop ((index 0))
          (if (= index prefix-length)
              #t
              (if (char=? (string-ref prefix index) (string-ref text index))
                  (loop (+ index 1))
                  #f))))))

(defun %string-index (text target)
  (let ((text-length (string-length text)))
    (let loop ((index 0))
      (if (>= index text-length)
          #f
          (if (char=? (string-ref text index) target)
              index
              (loop (+ index 1)))))))

(defun %assoc-value (alist key)
  (let ((entry (assoc key alist)))
    (if entry (cdr entry) #f)))

(defun alist-get (alist key)
  (%assoc-value alist key))

(defun %alist-set (alist key value)
  (let loop ((rest alist) (result '()) (replaced? #f))
    (if (null? rest)
        (reverse
          (if replaced?
              result
              (cons (cons key value) result)))
        (let ((entry (car rest)))
          (if (equal? key (car entry))
              (loop
                (cdr rest)
                (cons (cons key value) result)
                #t)
              (loop
                (cdr rest)
                (cons entry result)
                replaced?))))))

(defun %script-args-spec ()
  (if openwalk-script-meta
      (let ((entry (assoc "args" openwalk-script-meta)))
        (if entry (cdr entry) '()))
      '()))

(defun %arg-spec-name (arg-spec)
  (%assoc-value arg-spec "name"))

(defun %arg-spec-type (arg-spec)
  (let ((value (%assoc-value arg-spec "type")))
    (if value value "string")))

(defun %arg-spec-required? (arg-spec)
  (let ((entry (assoc "required" arg-spec)))
    (if entry (cdr entry) #f)))

(defun %arg-spec-default-entry (arg-spec)
  (assoc "default" arg-spec))

(defun %find-arg-spec (spec name)
  (let loop ((rest spec))
    (if (null? rest)
        #f
        (let ((arg-spec (car rest)))
          (if (equal? name (%arg-spec-name arg-spec))
              arg-spec
              (loop (cdr rest)))))))

(defun %ascii-downcase-char (ch)
  (if (and (char>=? ch #\A) (char<=? ch #\Z))
      (integer->char (+ (char->integer ch) 32))
      ch))

(defun %string-downcase-ascii (text)
  (list->string (map %ascii-downcase-char (string->list text))))

(defun %char-code=? (ch code)
  (= (char->integer ch) code))

(defun %boolean-type? (arg-type)
  (or (equal? arg-type "bool")
      (equal? arg-type "boolean")))

(defun %coerce-boolean (name value)
  (if (boolean? value)
      value
      (if (string? value)
          (let ((normalized (%string-downcase-ascii value)))
            (cond
              ((or (equal? normalized "true")
                   (equal? normalized "1")
                   (equal? normalized "yes")
                   (equal? normalized "on"))
               #t)
              ((or (equal? normalized "false")
                   (equal? normalized "0")
                   (equal? normalized "no")
                   (equal? normalized "off"))
               #f)
              (else
                (%parse-args-error
                  (string-append
                    "Invalid boolean value for `"
                    name
                    "`: "
                    value)))))
          (%parse-args-error
            (string-append
              "Invalid boolean value for `"
              name
              "`")))))

(defun %coerce-number (name value)
  (if (number? value)
      value
      (if (string? value)
          (let ((parsed (string->number value)))
            (if parsed
                parsed
                (%parse-args-error
                  (string-append
                    "Invalid number value for `"
                    name
                    "`: "
                    value))))
          (%parse-args-error
            (string-append
              "Invalid number value for `"
              name
              "`")))))

(defun %coerce-string (name value)
  (if (string? value)
      value
      (%parse-args-error
        (string-append
          "Invalid string value for `"
          name
          "`"))))

(defun %coerce-arg-value (arg-spec value)
  (let ((name (%arg-spec-name arg-spec))
        (arg-type (%arg-spec-type arg-spec)))
    (cond
      ((equal? arg-type "string")
       (%coerce-string name value))
      ((or (equal? arg-type "number")
           (equal? arg-type "integer"))
       (%coerce-number name value))
      ((%boolean-type? arg-type)
       (%coerce-boolean name value))
      (else
        (%parse-args-error
          (string-append
            "Unsupported argument type for `"
            name
            "`: "
            arg-type))))))

(defun %long-option? (token)
  (and (string? token)
       (>= (string-length token) 2)
       (%string-prefix? "--" token)))

(defun %parse-cli-args (args)
  (let loop ((rest args) (positionals '()) (named '()) (options-ended? #f))
    (if (null? rest)
        (list
          (cons "positionals" (reverse positionals))
          (cons "named" named))
        (let ((token (car rest)))
          (if options-ended?
              (loop (cdr rest) (cons token positionals) named #t)
              (cond
                ((equal? token "--")
                 (loop (cdr rest) positionals named #t))
                ((%long-option? token)
                 (let ((separator-index (%string-index token #\=)))
                   (if separator-index
                       (let ((name (substring token 2 separator-index))
                             (value
                               (substring
                                 token
                                 (+ separator-index 1)
                                 (string-length token))))
                         (if (= (string-length name) 0)
                             (%parse-args-error
                               (string-append
                                 "Invalid option token: "
                                 token))
                             (loop
                               (cdr rest)
                               positionals
                               (%alist-set named name value)
                               #f)))
                       (if (null? (cdr rest))
                           (%parse-args-error
                             (string-append
                               "Missing value for option: "
                               token))
                           (let ((name (substring token 2 (string-length token)))
                                 (value (cadr rest)))
                             (if (= (string-length name) 0)
                                 (%parse-args-error
                                   (string-append
                                     "Invalid option token: "
                                     token))
                                 (if (equal? value "--")
                                     (%parse-args-error
                                       (string-append
                                         "Missing value for option: "
                                         token))
                                     (if (%long-option? value)
                                         (%parse-args-error
                                           (string-append
                                             "Missing value for option: "
                                             token))
                                         (loop
                                           (cddr rest)
                                           positionals
                                           (%alist-set named name value)
                                           #f)))))))))
                (else
                  (loop (cdr rest) (cons token positionals) named #f))))))))

(defun %apply-positional-args (spec positionals)
  (let loop ((spec-rest spec) (value-rest positionals) (result '()))
    (if (null? value-rest)
        (reverse result)
        (if (null? spec-rest)
            (%parse-args-error
              (string-append
                "Unexpected positional argument: "
                (car value-rest)))
            (let ((arg-spec (car spec-rest)))
              (loop
                (cdr spec-rest)
                (cdr value-rest)
                (cons
                  (cons
                    (%arg-spec-name arg-spec)
                    (%coerce-arg-value arg-spec (car value-rest)))
                  result)))))))

(defun %apply-default-args (spec parsed)
  (let loop ((spec-rest spec) (result parsed))
    (if (null? spec-rest)
        result
        (let* ((arg-spec (car spec-rest))
               (name (%arg-spec-name arg-spec))
               (default-entry (%arg-spec-default-entry arg-spec)))
          (if (or (assoc name result) (not default-entry))
              (loop (cdr spec-rest) result)
              (loop
                (cdr spec-rest)
                (%alist-set
                  result
                  name
                  (%coerce-arg-value arg-spec (cdr default-entry)))))))))

(defun %apply-named-args (spec named parsed)
  (let loop ((rest named) (result parsed))
    (if (null? rest)
        result
        (let* ((entry (car rest))
               (name (car entry))
               (value (cdr entry))
               (arg-spec (%find-arg-spec spec name)))
          (if (not arg-spec)
              (%parse-args-error
                (string-append
                  "Unknown argument: --"
                  name))
              (loop
                (cdr rest)
                (%alist-set
                  result
                  name
                  (%coerce-arg-value arg-spec value))))))))

(defun %validate-required-args (spec parsed)
  (let loop ((spec-rest spec))
    (if (null? spec-rest)
        parsed
        (let* ((arg-spec (car spec-rest))
               (name (%arg-spec-name arg-spec)))
          (if (and (%arg-spec-required? arg-spec)
                   (not (assoc name parsed)))
              (%parse-args-error
                (string-append
                  "Missing required argument: "
                  name))
              (loop (cdr spec-rest)))))))

(defun %parse-args (args spec)
  (let* ((parsed-cli (%parse-cli-args args))
         (positionals (%assoc-value parsed-cli "positionals"))
         (named (%assoc-value parsed-cli "named"))
         (with-positionals (%apply-positional-args spec positionals))
         (with-defaults (%apply-default-args spec with-positionals))
         (with-named (%apply-named-args spec named with-defaults)))
    (%validate-required-args spec with-named)))

(defun %js-escape-string (text)
  (let loop ((chars (string->list text)) (parts '()))
    (if (null? chars)
        (apply string-append (reverse parts))
        (let ((ch (car chars)))
          (loop
            (cdr chars)
            (cons
              (cond
                ((%char-code=? ch 92) "\\\\")
                ((%char-code=? ch 34) "\\\"")
                ((%char-code=? ch 10) "\\n")
                ((%char-code=? ch 13) "\\r")
                ((%char-code=? ch 9) "\\t")
                (else (string ch)))
              parts))))))

(defun %scheme-value->js-literal (value)
  (cond
    ((string? value)
     (string-append "\"" (%js-escape-string value) "\""))
    ((boolean? value)
     (if value "true" "false"))
    ((number? value)
     (number->string value))
    (else "null")))

(defun %string-join (items separator)
  (if (null? items)
      ""
      (let loop ((rest (cdr items)) (result (car items)))
        (if (null? rest)
            result
            (loop
              (cdr rest)
              (string-append result separator (car rest)))))))

(defun %parsed-args->js-object-literal (parsed spec)
  (let loop ((spec-rest spec) (parts '()))
    (if (null? spec-rest)
        (string-append "{" (%string-join (reverse parts) ", ") "}")
        (let* ((arg-spec (car spec-rest))
               (name (%arg-spec-name arg-spec))
               (entry (assoc name parsed)))
          (if entry
              (loop
                (cdr spec-rest)
                (cons
                  (string-append
                    "\""
                    (%js-escape-string name)
                    "\": "
                    (%scheme-value->js-literal (cdr entry)))
                  parts))
              (loop (cdr spec-rest) parts))))))

(defun %args->js-object-literal (args spec)
  (%parsed-args->js-object-literal (%parse-args args spec) spec))

(define-syntax parse-args
  (syntax-rules ()
    ((_ args)
     (%parse-args args (%script-args-spec)))
    ((_ args spec)
     (%parse-args args spec))))

(defun js-run (filename args)
  (js-eval
    (string-append
      "("
      (read-sibling-file filename)
      ")("
      (%args->js-object-literal args (%script-args-spec))
      ")")))

(defun call-with-input-file (path proc)
  (let ((port (open-input-file path)))
    (dynamic-wind
      (lambda () #f)
      (lambda ()
        (proc port))
      (lambda ()
        (close-input-port port)))))

(defun call-with-output-file (path proc)
  (let ((port (open-output-file path)))
    (dynamic-wind
      (lambda () #f)
      (lambda ()
        (proc port))
      (lambda ()
        (close-output-port port)))))
"#;
