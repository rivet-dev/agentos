#include <strings.h>
#ifdef strncasecmp_l
#undef strncasecmp_l
#endif
int (*foo)(const char *, const char *, size_t, locale_t) = strncasecmp_l;
int main(void) { return 0; }
