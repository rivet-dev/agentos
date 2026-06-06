#include <string.h>
#ifdef strlen
#undef strlen
#endif
size_t (*foo)(const char *) = strlen;
int main(void) { return 0; }
