#include <wordexp.h>
#ifdef wordfree
#undef wordfree
#endif
void (*foo)(wordexp_t *) = wordfree;
int main(void) { return 0; }
