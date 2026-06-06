#include <glob.h>
#ifdef globfree
#undef globfree
#endif
void (*foo)(glob_t *) = globfree;
int main(void) { return 0; }
