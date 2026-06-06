#include <string.h>
#ifdef strspn
#undef strspn
#endif
size_t (*foo)(const char *, const char *) = strspn;
int main(void) { return 0; }
