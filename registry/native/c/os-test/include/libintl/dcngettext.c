#include <libintl.h>
#ifdef dcngettext
#undef dcngettext
#endif
char *(*foo)(const char *, const char *, const char *, unsigned long int, int) = dcngettext;
int main(void) { return 0; }
