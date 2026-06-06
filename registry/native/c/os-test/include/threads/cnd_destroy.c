#include <threads.h>
#ifdef cnd_destroy
#undef cnd_destroy
#endif
void (*foo)(cnd_t *) = cnd_destroy;
int main(void) { return 0; }
