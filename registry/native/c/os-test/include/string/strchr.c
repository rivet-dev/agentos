#include <string.h>
#ifdef strchr
#undef strchr
#endif
char *(*foo)(const char *, int) = strchr;
int main(void) { return 0; }
