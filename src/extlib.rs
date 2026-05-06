pub const LIB: &str = r#"
;; Minimal helpers shared by Scheme tools.
;;
;; Usage:
;;   (def answer 42)
;;   (def (add a b) (+ a b))
;;   (defun add (a b) (+ a b))
;;   (open "https://example.com")

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

(defun open (url)
  (let ((session-name openwalk-session-name)
        (sessions (vector->list(browser-list))))
    (if session-name
        (if (member session-name sessions)
            (tab-new url)
            (browser-open url))
        (with-exception-handler
          (lambda (ex)
            (browser-open url))
          (lambda ()
            (tab-new url))))))

"#;
