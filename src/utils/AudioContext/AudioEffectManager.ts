/**
 * AudioEffectManager - Manages spectrum analysis and audio effects
 * Integrates LowFreqVolumeAnalyzer for bass detection
 */

import { LowFreqVolumeAnalyzer } from './LowFreqVolumeAnalyzer';

/**
 * AudioEffectManager - 管理频谱分析
 * Based on dev branch implementation with integrated LowFreqVolumeAnalyzer
 */
export class AudioEffectManager {
  private audioCtx: AudioContext;
  private analyserNode: AnalyserNode | null = null;
  private lowFreqAnalyzer: LowFreqVolumeAnalyzer;

  constructor(audioCtx: AudioContext) {
    this.audioCtx = audioCtx;
    this.lowFreqAnalyzer = new LowFreqVolumeAnalyzer({
      binCount: 3,
      smoothFactor: 0.28,
      threshold: 180,
      powerExponent: 2,
    });
    this.initNodes();
  }

  private initNodes(): void {
    this.analyserNode = this.audioCtx.createAnalyser();
    this.analyserNode.fftSize = 2048;
    this.analyserNode.smoothingTimeConstant = 0.78;
  }

  /**
   * Connect input node through effect chain
   * @param inputNode The input audio node
   * @returns The output audio node
   */
  connect(inputNode: AudioNode): AudioNode {
    if (this.analyserNode) {
      inputNode.connect(this.analyserNode);
      return this.analyserNode;
    }
    return inputNode;
  }

  /**
   * Get frequency data for spectrum visualization
   * @returns Uint8Array containing frequency data
   */
  getFrequencyData(): Uint8Array {
    if (!this.analyserNode) return new Uint8Array(0);
    const dataArray = new Uint8Array(this.analyserNode.frequencyBinCount);
    this.analyserNode.getByteFrequencyData(dataArray);
    return dataArray;
  }

  /**
   * Get smoothed low frequency volume for background effects
   * Uses LowFreqVolumeAnalyzer for consistent calculation
   * @returns number in 0-1 range
   */
  getLowFrequencyVolume(): number {
    if (!this.analyserNode) return 0;

    const dataArray = new Uint8Array(this.analyserNode.frequencyBinCount);
    this.analyserNode.getByteFrequencyData(dataArray);

    return this.lowFreqAnalyzer.analyze(Array.from(dataArray));
  }

  /**
   * Disconnect all nodes
   */
  disconnect(): void {
    this.analyserNode?.disconnect();
    this.lowFreqAnalyzer.reset();
  }
}
