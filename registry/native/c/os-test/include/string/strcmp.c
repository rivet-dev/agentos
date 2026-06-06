#include <string.h>
#ifdef strcmp
#undef strcmp
#endif
int (*foo)(const char *, const char *) = strcmp;
int main(void) { return 0; }
