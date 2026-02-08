// 定义一个通用的类型，用于指定分块后的子数组类型
pub type ChunkedArray<T> = Vec<Vec<T>>;

/**
 * 将输入数组按照指定的大小进行分块。
 *
 * @param input - 需要被分块的原始数组。
 * @param size - 每个块应该包含的元素数量。
 * @returns 一个包含按指定大小分组的子数组的新数组。
 */
pub fn chunk<T>(input: &[T], size: usize) -> ChunkedArray<T>
where
    T: Clone,
{
    input.chunks(size).map(|chunk| chunk.to_vec()).collect()
}
