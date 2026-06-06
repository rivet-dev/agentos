#include <libintl.h>
#ifdef bind_textdomain_codeset
#undef bind_textdomain_codeset
#endif
char *(*foo)(const char *, const char *) = bind_textdomain_codeset;
int main(void) { return 0; }
