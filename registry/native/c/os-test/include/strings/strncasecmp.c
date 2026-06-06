#include <strings.h>
#ifdef strncasecmp
#undef strncasecmp
#endif
int (*foo)(const char *, const char *, size_t) = strncasecmp;
int main(void) { return 0; }
