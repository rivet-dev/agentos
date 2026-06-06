#include <string.h>
#ifdef strcoll
#undef strcoll
#endif
int (*foo)(const char *, const char *) = strcoll;
int main(void) { return 0; }
