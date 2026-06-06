#include <strings.h>
#ifdef strcasecmp
#undef strcasecmp
#endif
int (*foo)(const char *, const char *) = strcasecmp;
int main(void) { return 0; }
