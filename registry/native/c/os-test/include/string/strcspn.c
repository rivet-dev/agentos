#include <string.h>
#ifdef strcspn
#undef strcspn
#endif
size_t (*foo)(const char *, const char *) = strcspn;
int main(void) { return 0; }
