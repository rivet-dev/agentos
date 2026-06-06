#include <string.h>
#ifdef strncmp
#undef strncmp
#endif
int (*foo)(const char *, const char *, size_t) = strncmp;
int main(void) { return 0; }
