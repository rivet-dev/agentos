#include <strings.h>
#ifdef strcasecmp_l
#undef strcasecmp_l
#endif
int (*foo)(const char *, const char *, locale_t) = strcasecmp_l;
int main(void) { return 0; }
